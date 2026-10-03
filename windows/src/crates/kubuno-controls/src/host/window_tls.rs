//! Several host windows on one UI thread (a main window, its modal dialogs, other forms).
//!
//! The host keeps what a frame reads and writes in thread-locals (the input queue, the cursor, the
//! repaint delay, the close handler, the title bar…): the page has no handle to its window, it just
//! calls `host::events()`, `host::request_repaint_after(…)`. With one window per thread that state is
//! the window's. With several, each window keeps its own copy, and the one that belongs to the window
//! a message is for is **installed** in the thread-locals while the window procedure runs, then the
//! previous one is put back ([`enter`] and its guard). Nested calls (a modal dialog opened from a
//! frame of its owner, a message sent to another window from a handler) follow a stack discipline, so
//! the owner's frame finds its own state again when the dialog's loop returns.
//!
//! Before any window exists (and between windows), the thread-locals hold the "detached" state:
//! what an application registers before calling `run_with_options` (`on_message`,
//! `set_close_handler`, `on_ready`). The first window created from there adopts it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use windows::Win32::Foundation::HWND;

use super::form::{CaptionButtonState, FormOptions};
use super::{caption, dnd, form, input, CaptionHot, CloseFn, MsgHandler, OverlayReq, ReadyFn, TitleBar};
use drive_app_controls::{Rect, Theme};

/// Everything per-window the host keeps in thread-locals.
pub(super) struct WindowTls {
    input: input::WindowState,
    dnd: dnd::WindowState,
    caption_buttons: (CaptionButtonState, CaptionButtonState, bool),
    pending_form: Option<FormOptions>,
    access: Option<super::access::AccessTree>,
    overlays: Vec<OverlayReq>,
    content_offset: (f32, f32),
    last_popups: Vec<Rect>,
    title_bar: TitleBar,
    caption_hot: Option<CaptionHot>,
    handlers: HashMap<u32, Vec<MsgHandler>>,
    close_handler: Option<CloseFn>,
    pub(super) quit_requested: bool,
    pending_theme: Option<Theme>,
    pending_font: Option<Option<String>>,
    on_ready: Option<ReadyFn>,
    pub(super) current_font: Option<String>,
    accept_files: bool,
    accept_files_last: bool,
    parent_hwnd: isize,
    chrome: super::chrome::ChromeTls,
}

impl Default for WindowTls {
    fn default() -> Self {
        Self {
            input: input::WindowState::default(),
            dnd: dnd::WindowState::default(),
            caption_buttons: (CaptionButtonState::Shown, CaptionButtonState::Shown, true),
            pending_form: None,
            access: None,
            overlays: Vec::new(),
            content_offset: (0.0, 0.0),
            last_popups: Vec::new(),
            title_bar: TitleBar::default(),
            caption_hot: None,
            handlers: HashMap::new(),
            close_handler: None,
            quit_requested: false,
            pending_theme: None,
            pending_font: None,
            on_ready: None,
            current_font: None,
            accept_files: false,
            accept_files_last: false,
            parent_hwnd: 0,
            chrome: Default::default(),
        }
    }
}

impl WindowTls {
    /// A new window's state with its caption buttons already chosen.
    pub(super) fn with_caption_buttons(mut self, buttons: (CaptionButtonState, CaptionButtonState, bool)) -> Self {
        self.caption_buttons = buttons;
        self
    }

    /// Moves the installed state out of the thread-locals, leaving the defaults.
    fn take() -> Self {
        fn take_ref<T: Default>(cell: &'static std::thread::LocalKey<RefCell<T>>) -> T {
            cell.with(|c| c.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default())
        }
        Self {
            input: input::take_window_state(),
            dnd: dnd::take_window_state(),
            caption_buttons: caption::take_window_state(),
            pending_form: form::take_pending(),
            access: super::access::take_published(),
            overlays: take_ref(&super::OVERLAYS),
            content_offset: super::CONTENT_OFFSET.with(|c| c.replace((0.0, 0.0))),
            last_popups: take_ref(&super::LAST_POPUPS),
            title_bar: take_ref(&super::TITLE_BAR),
            caption_hot: super::CAPTION_HOT.with(Cell::take),
            handlers: take_ref(&super::HANDLERS),
            close_handler: take_ref(&super::CLOSE_HANDLER),
            quit_requested: super::QUIT_REQUESTED.with(|c| c.replace(false)),
            pending_theme: take_ref(&super::PENDING_THEME),
            pending_font: take_ref(&super::PENDING_FONT),
            on_ready: take_ref(&super::ON_READY),
            current_font: take_ref(&super::CURRENT_FONT),
            accept_files: super::ACCEPT_FILES.with(|c| c.replace(false)),
            accept_files_last: super::ACCEPT_FILES_LAST.with(|c| c.replace(false)),
            parent_hwnd: super::PARENT_HWND.with(|c| c.replace(0)),
            chrome: super::chrome::take_window_state(),
        }
    }

    /// Installs this state into the thread-locals.
    fn put(self) {
        fn put_ref<T>(cell: &'static std::thread::LocalKey<RefCell<T>>, value: T) {
            cell.with(|c| {
                if let Ok(mut slot) = c.try_borrow_mut() {
                    *slot = value;
                }
            });
        }
        input::put_window_state(self.input);
        dnd::put_window_state(self.dnd);
        caption::set_buttons(self.caption_buttons);
        form::put_pending(self.pending_form);
        super::access::put_published(self.access);
        put_ref(&super::OVERLAYS, self.overlays);
        super::CONTENT_OFFSET.with(|c| c.set(self.content_offset));
        put_ref(&super::LAST_POPUPS, self.last_popups);
        put_ref(&super::TITLE_BAR, self.title_bar);
        super::CAPTION_HOT.with(|c| c.set(self.caption_hot));
        put_ref(&super::HANDLERS, self.handlers);
        put_ref(&super::CLOSE_HANDLER, self.close_handler);
        super::QUIT_REQUESTED.with(|c| c.set(self.quit_requested));
        put_ref(&super::PENDING_THEME, self.pending_theme);
        put_ref(&super::PENDING_FONT, self.pending_font);
        put_ref(&super::ON_READY, self.on_ready);
        put_ref(&super::CURRENT_FONT, self.current_font);
        super::ACCEPT_FILES.with(|c| c.set(self.accept_files));
        super::ACCEPT_FILES_LAST.with(|c| c.set(self.accept_files_last));
        super::PARENT_HWND.with(|c| c.set(self.parent_hwnd));
        super::chrome::put_window_state(self.chrome);
    }
}

thread_local! {
    /// The window whose state is installed (`0`: the detached state, no window).
    static ACTIVE: Cell<isize> = const { Cell::new(0) };
    /// The states of the other windows (and the detached one, key `0`) while they are not installed.
    static SAVED: RefCell<HashMap<isize, WindowTls>> = RefCell::new(HashMap::new());
    /// The state the next window created adopts (set just before `CreateWindowExW`).
    static NEXT: RefCell<Option<WindowTls>> = const { RefCell::new(None) };
    /// Windows that got `WM_NCDESTROY`: their state is dropped when their procedure returns.
    static DESTROYED: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new());
}

/// `hwnd` got its last message (`WM_NCDESTROY`): its state goes when the procedure returns.
pub(super) fn destroyed(hwnd: HWND) {
    DESTROYED.with(|d| d.borrow_mut().insert(hwnd.0 as isize));
}

/// Whether some window's state is installed (the call runs inside a window procedure or a window's
/// loop): a window opened from here is a nested one (a dialog opened from a frame).
pub(super) fn in_window() -> bool {
    ACTIVE.with(Cell::get) != 0
}

/// Prepares the state the next window created will adopt: the detached state (what the application
/// registered before opening its first window) when `adopt_detached` and no window's state is
/// installed, else a fresh one (a dialog opened from a frame of its owner must not take the owner's
/// handlers).
pub(super) fn prepare_next(adopt_detached: bool, configure: impl FnOnce(WindowTls) -> WindowTls) {
    let base = if adopt_detached && !in_window() { WindowTls::take() } else { WindowTls::default() };
    let next = configure(base);
    NEXT.with(|n| *n.borrow_mut() = Some(next));
}

/// Drops a prepared state nobody adopted (the window could not be created).
pub(super) fn discard_next() {
    NEXT.with(|n| n.borrow_mut().take());
}

/// Installs `target`'s state (saving the installed one).
fn switch_to(target: isize) {
    let current = ACTIVE.with(Cell::get);
    if current == target {
        return;
    }
    let state = WindowTls::take();
    let next = SAVED.with(|s| {
        let mut saved = s.borrow_mut();
        saved.insert(current, state);
        saved.remove(&target)
    });
    let next = next.or_else(|| if target == 0 { None } else { NEXT.with(|n| n.borrow_mut().take()) }).unwrap_or_default();
    next.put();
    ACTIVE.with(|a| a.set(target));
}

/// Restores the previous state when dropped (see [`enter`]).
#[must_use]
pub(super) struct Guard {
    hwnd: isize,
    previous: isize,
    switched: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.switched {
            switch_to(self.previous);
        }
        // A window destroyed meanwhile keeps no saved state (a later window may reuse its handle).
        if self.hwnd != 0 && ACTIVE.with(Cell::get) != self.hwnd && DESTROYED.with(|d| d.borrow_mut().remove(&self.hwnd)) {
            SAVED.with(|s| s.borrow_mut().remove(&self.hwnd));
        }
    }
}

/// Installs `hwnd`'s state for the duration of the returned guard (the window procedure's and the
/// window's loop's). A window seen for the first time adopts the state prepared by
/// [`prepare_next`], or a fresh one.
pub(super) fn enter(hwnd: HWND) -> Guard {
    let raw = hwnd.0 as isize;
    let previous = ACTIVE.with(Cell::get);
    let switched = previous != raw;
    if switched {
        switch_to(raw);
    }
    Guard { hwnd: raw, previous, switched }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_window_keeps_its_own_state_and_nesting_restores_the_outer_one() {
        // The detached state (before any window): a pending repaint request.
        crate::host::input::request_repaint_after(40);
        prepare_next(true, |s| s);
        let a = HWND(0x1111 as *mut _);
        let b = HWND(0x2222 as *mut _);
        {
            let _ga = enter(a);
            // `a` adopted the detached state.
            assert_eq!(super::input::take_repaint_after(), Some(40));
            crate::host::input::request_repaint_after(7);
            {
                // A nested window (a dialog opened from a frame of `a`) starts fresh.
                prepare_next(true, |s| s);
                let _gb = enter(b);
                assert_eq!(super::input::take_repaint_after(), None);
                crate::host::input::request_repaint_after(99);
            }
            // Back in `a`: its own request, not `b`'s.
            assert_eq!(super::input::take_repaint_after(), Some(7));
            {
                let _gb = enter(b);
                // `b` kept its state while `a` ran (the handles are not real windows, so the
                // guard does not forget them).
                assert_eq!(super::input::take_repaint_after(), Some(99));
            }
        }
        assert!(!in_window());
        SAVED.with(|s| s.borrow_mut().clear());
    }
}
