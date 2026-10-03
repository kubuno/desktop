//! Drives a [`crate::compile::CompiledView`] across frames, and reloads it —
//! `XML_VIEWS.md` §5: "a parse or a registry-lookup error must not blank the
//! screen. The loader keeps the **last successfully compiled plan** and keeps
//! painting it, while showing a small in-app banner". [`Runtime`] is that
//! loader: [`Runtime::reload_from_text`] is the "parse+compile, once per file
//! change" half, [`Runtime::frame`] is the "bind+paint, every frame" half.
//!
//! File-watching itself is not here — this module works on `&str`, so it can
//! be unit-tested without a filesystem (see this module's tests) or a `.
//! kbview` file at all. [`FileWatcher`] is the thin wrapper the preview
//! example actually drives: it reads the file and calls
//! [`Runtime::reload_from_text`] when the mtime moves.

//!
//! ## Lifecycle, threads and async (EVT-6, `vskubuno/docs/EVENTS.md` §3/§6)
//!
//! A runtime is also its view's "form": it raises Load → Activated → Shown when the view first
//! appears, and handles closing — every routed frame declares `host::defer_close`, so the
//! window's close button, Alt+F4, `host::close_window`, [`Runtime::close`] or the end of the
//! Windows session reach the next frame, which raises FormClosing (cancelable: the window stays)
//! then FormClosed → Deactivate, shuts the view's services down and quits the host loop. It
//! also owns the view's UI-thread services: [`Runtime::dispatcher`] (Invoke/BeginInvoke from
//! worker threads), [`Runtime::spawn_local`] and async handlers (an executor polled from the
//! message loop), and [`Runtime::add_timer`] (WinForms `Timer`). A frame runs, in order:
//!
//! 1. a pending close request → FormClosing, and when nobody cancelled it FormClosed →
//!    Deactivate → [`Runtime::shutdown`] → `host::quit` (the frame then only paints);
//! 2. the input router's first half (Load → Activated on the first frame, input events);
//! 3. the closures posted by [`UiDispatcher`]s, in posting order, then the due timer ticks,
//!    then the async tasks that were woken (their `UiHandle::update`s see this frame's state);
//! 4. the paint (controls raise their own events), the router's second half (Shown);
//! 5. the tasks spawned during this frame (their first poll), and the wake-up of the next due
//!    timer or delay (`host::request_wake_after`, which also runs while minimised).

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use kubuno_controls::host::{self, Frame, InputEvent};
use kubuno_controls::ControlCanvas;

use crate::binding::{HandlerTable, ViewModel};
use crate::compile::{self, CompiledView};
use crate::events::dispatcher::UiShared;
use crate::events::executor::{self, UiContext};
use crate::events::router::{Dispatch, FrameInput, InputRouter};
use crate::events::{
    CloseReason, Event, EventSink, FormClosedEventArgs, FormClosingEventArgs, JoinHandle, Timer, TypedViewModel, UiDispatcher, UiHandle,
};
use crate::events::ElementRef;
use crate::node::{PaintCx, ViewEvent};
use crate::scope::{ComponentScope, ErrorBlinkStyle, ErrorIconAlignment, ScopedViewModel};
use crate::syntax::Diagnostic;
use kubuno_ui::{FocusRing, Rect};

/// The host services a runtime uses — `kubuno_controls::host`'s thread-local functions in an
/// application, a scripted fake in this crate's tests.
pub(crate) trait HostPort {
    /// This frame's unconsumed input events.
    fn events(&self) -> Vec<InputEvent>;
    /// Consumes the first unconsumed event equal to `target`.
    fn consume(&self, target: &InputEvent);
    fn now_ms(&self) -> u64;
    fn request_repaint_after(&self, ms: u32);
    fn request_wake_after(&self, ms: u32);
    fn defer_close(&self);
    fn cancel_close(&self);
    fn quit(&self);
    /// The pointer shape of this frame (`crate::common`: `Cursor`, `UseWaitCursor`).
    fn set_cursor(&self, cursor: host::Cursor);
    /// The window's `Form` properties (`crate::window`).
    fn set_form(&self, form: host::FormOptions);
    /// The accessibility tree of this frame.
    fn publish_access(&self, tree: host::access::AccessTree);
    /// What assistive technology asked since the last frame.
    fn take_access_actions(&self) -> Vec<(u64, host::access::AccessAction)>;
    /// Whether files may be dropped on the window this frame.
    fn accept_files(&self, accept: bool);
    /// The drag over the window this frame (EVT-8), if any.
    fn drag(&self) -> Option<host::dnd::DragFrame>;
    /// The answer to the drag: what a drop at the pointer would do.
    fn set_drag_effect(&self, effect: crate::events::DragDropEffects);
    /// What the window's chrome did since the last frame (`host::WindowEvent`).
    fn window_events(&self) -> Vec<host::WindowEvent> {
        Vec::new()
    }
    /// Whether the window is on screen this frame: a window started hidden runs its first frames
    /// off screen (it gets `Load` there), and `Shown` waits until it is actually shown.
    fn window_visible(&self) -> bool {
        host::window_visible()
    }
}

/// The real host (`kubuno_controls::host`).
struct RealHost;

impl HostPort for RealHost {
    fn events(&self) -> Vec<InputEvent> {
        host::events()
    }

    fn consume(&self, target: &InputEvent) {
        let mut done = false;
        host::consume(|e| {
            let hit = !done && e == target;
            done |= hit;
            hit
        });
    }

    fn now_ms(&self) -> u64 {
        host::now_ms()
    }

    fn request_repaint_after(&self, ms: u32) {
        host::request_repaint_after(ms);
    }

    fn request_wake_after(&self, ms: u32) {
        host::request_wake_after(ms);
    }

    fn defer_close(&self) {
        host::defer_close();
    }

    fn cancel_close(&self) {
        host::cancel_close();
    }

    fn quit(&self) {
        host::quit();
    }

    fn set_cursor(&self, cursor: host::Cursor) {
        host::set_cursor(cursor);
    }

    fn set_form(&self, form: host::FormOptions) {
        host::set_form(form);
    }

    fn window_events(&self) -> Vec<host::WindowEvent> {
        host::take_window_events()
    }

    fn publish_access(&self, tree: host::access::AccessTree) {
        host::access::publish(tree);
    }

    fn take_access_actions(&self) -> Vec<(u64, host::access::AccessAction)> {
        host::access::take_actions()
    }

    fn accept_files(&self, accept: bool) {
        host::accept_files(accept);
    }

    fn drag(&self) -> Option<host::dnd::DragFrame> {
        host::dnd::current()
    }

    fn set_drag_effect(&self, effect: crate::events::DragDropEffects) {
        host::dnd::set_effect(effect);
    }
}

/// The host as seen by a view drawn INSIDE another view's window (an MDI document, an in-window
/// dialog): it shares the window's input, cursor and timers, but the window itself — its `Form`
/// properties, its close, its accessibility tree, its chrome events — belongs to the outer view.
struct InnerHost;

impl HostPort for InnerHost {
    fn events(&self) -> Vec<InputEvent> {
        host::events().into_iter().filter(|e| !matches!(e, InputEvent::CloseRequested(_))).collect()
    }

    fn consume(&self, target: &InputEvent) {
        RealHost.consume(target);
    }

    fn now_ms(&self) -> u64 {
        host::now_ms()
    }

    fn request_repaint_after(&self, ms: u32) {
        host::request_repaint_after(ms);
    }

    fn request_wake_after(&self, ms: u32) {
        host::request_wake_after(ms);
    }

    fn defer_close(&self) {}

    fn cancel_close(&self) {}

    fn quit(&self) {}

    fn set_cursor(&self, cursor: host::Cursor) {
        host::set_cursor(cursor);
    }

    fn set_form(&self, _form: host::FormOptions) {}

    fn publish_access(&self, _tree: host::access::AccessTree) {}

    fn take_access_actions(&self) -> Vec<(u64, host::access::AccessAction)> {
        Vec::new()
    }

    fn accept_files(&self, _accept: bool) {}

    fn drag(&self) -> Option<host::dnd::DragFrame> {
        None
    }

    fn set_drag_effect(&self, _effect: crate::events::DragDropEffects) {}
}

fn close_reason(reason: host::CloseReason) -> CloseReason {
    match reason {
        host::CloseReason::UserClosing => CloseReason::UserClosing,
        host::CloseReason::ApplicationExitCall => CloseReason::ApplicationExitCall,
        host::CloseReason::WindowsShutDown => CloseReason::WindowsShutDown,
        host::CloseReason::TaskManagerClosing => CloseReason::TaskManagerClosing,
    }
}

/// How a frame reaches the view model: untyped ([`Runtime::frame`]), or typed
/// ([`Runtime::frame_typed`]), which also gives posted closures and async tasks `&mut V`.
trait FrameVm {
    fn with_vm<R>(&mut self, f: impl FnOnce(&mut dyn ViewModel) -> R) -> R;
    /// Runs the closures posted by `UiDispatcher`s.
    fn run_jobs(&mut self, ui: &UiContext);
    /// Polls the woken async tasks; returns how many polls ran.
    fn run_tasks(&mut self, ui: &UiContext) -> usize;
}

struct DynVm<'a>(&'a mut dyn ViewModel);

impl FrameVm for DynVm<'_> {
    fn with_vm<R>(&mut self, f: impl FnOnce(&mut dyn ViewModel) -> R) -> R {
        f(&mut *self.0)
    }

    fn run_jobs(&mut self, ui: &UiContext) {
        // No concrete type to hand over: each closure reports `DispatchError::WrongViewModel`.
        for job in ui.shared.take_jobs() {
            job(None);
        }
    }

    fn run_tasks(&mut self, ui: &UiContext) -> usize {
        ui.run_ready()
    }
}

struct TypedVm<'a, V>(&'a mut V);

impl<V: ViewModel + EventSink + 'static> FrameVm for TypedVm<'_, V> {
    fn with_vm<R>(&mut self, f: impl FnOnce(&mut dyn ViewModel) -> R) -> R {
        f(&mut TypedViewModel(&mut *self.0))
    }

    fn run_jobs(&mut self, ui: &UiContext) {
        for job in ui.shared.take_jobs() {
            job(Some(&mut *self.0));
        }
    }

    fn run_tasks(&mut self, ui: &UiContext) -> usize {
        executor::with_vm_scope(ui, &mut *self.0, || ui.run_ready())
    }
}

/// A frame's view model seen through the view's named components (DATA-2, `crate::scope`): the
/// binding paths that name a component go to it.
struct ScopedFrame<'s, F> {
    inner: &'s mut F,
    scope: ComponentScope,
    live: bool,
}

impl<F: FrameVm> FrameVm for ScopedFrame<'_, F> {
    fn with_vm<R>(&mut self, f: impl FnOnce(&mut dyn ViewModel) -> R) -> R {
        let scope = &self.scope;
        let live = self.live;
        self.inner.with_vm(|vm| {
            let mut scoped = ScopedViewModel::new(vm, scope, live);
            f(&mut scoped)
        })
    }

    fn run_jobs(&mut self, ui: &UiContext) {
        self.inner.run_jobs(ui);
    }

    fn run_tasks(&mut self, ui: &UiContext) -> usize {
        self.inner.run_tasks(ui)
    }
}

/// The size of an ErrorProvider glyph, DIP.
const GLYPH: f32 = 16.0;

/// Where the error glyph of a control whose box is `bounds` goes.
pub(crate) fn glyph_rect(bounds: Rect, alignment: ErrorIconAlignment, padding: f32) -> Rect {
    let x = match alignment {
        ErrorIconAlignment::TopLeft | ErrorIconAlignment::MiddleLeft | ErrorIconAlignment::BottomLeft => bounds.left - padding - GLYPH,
        _ => bounds.right + padding,
    };
    let y = match alignment {
        ErrorIconAlignment::TopLeft | ErrorIconAlignment::TopRight => bounds.top,
        ErrorIconAlignment::BottomLeft | ErrorIconAlignment::BottomRight => bounds.bottom - GLYPH,
        _ => ((bounds.top + bounds.bottom) / 2.0 - GLYPH / 2.0).round(),
    };
    Rect::new(x, y, x + GLYPH, y + GLYPH)
}

/// Whether a glyph shown since `elapsed` ms is visible now, and in how many ms that changes
/// (`None`: it no longer blinks).
pub(crate) fn glyph_phase(blink: ErrorBlinkStyle, rate: u32, elapsed: u64) -> (bool, Option<u32>) {
    let rate = u64::from(rate.max(50));
    let phase = elapsed / rate;
    let next = u32::try_from(rate - elapsed % rate).unwrap_or(u32::MAX);
    match blink {
        ErrorBlinkStyle::NeverBlink => (true, None),
        ErrorBlinkStyle::AlwaysBlink => (phase.is_multiple_of(2), Some(next)),
        // Three blinks when the glyph appears or its message changes, then steady (WinForms).
        ErrorBlinkStyle::BlinkIfDifferentError if phase >= 6 => (true, None),
        ErrorBlinkStyle::BlinkIfDifferentError => (phase.is_multiple_of(2), Some(next)),
    }
}

/// A view model that answers its own events ([`ViewModel::dispatch_event`]) and is handed to
/// posted closures and async tasks as its concrete type ([`Runtime::frame_model`]).
struct ModelVm<'a, V>(&'a mut V);

impl<V: ViewModel + 'static> FrameVm for ModelVm<'_, V> {
    fn with_vm<R>(&mut self, f: impl FnOnce(&mut dyn ViewModel) -> R) -> R {
        f(&mut *self.0)
    }

    fn run_jobs(&mut self, ui: &UiContext) {
        for job in ui.shared.take_jobs() {
            job(Some(&mut *self.0));
        }
    }

    fn run_tasks(&mut self, ui: &UiContext) -> usize {
        executor::with_vm_scope(ui, &mut *self.0, || ui.run_ready())
    }
}

/// What the designer shows after [`Runtime::reload_for_design`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignRenderState {
    /// The view as written: no error.
    Clean,
    /// The view with errors, its valid part shown (placeholders, defaults).
    Tolerant,
    /// A malformed text opened with no previous preview: what the parser recovered is shown.
    Recovered,
    /// A malformed text: the previous preview stays on screen.
    Stale,
    /// Nothing could be shown (no element at all).
    Empty,
}

/// The outcome of [`Runtime::reload_for_design`].
#[derive(Debug, Clone)]
pub struct DesignReload {
    pub state: DesignRenderState,
    /// The diagnostics of the text as written.
    pub diagnostics: Vec<Diagnostic>,
    /// The elements the preview shows differently from their text (for the surface's markers);
    /// empty for a stale preview (its ids are those of an older text).
    pub issues: Vec<crate::tolerant::DesignIssue>,
}

/// Owns the current compiled view (if any has ever compiled successfully),
/// the diagnostics of the *last* reload attempt (empty when it succeeded),
/// and the [`FocusRing`] a `.kbview` file's controls register with — the
/// controller §0/§5 says must survive a reload untouched.
pub struct Runtime {
    compiled: Option<CompiledView>,
    diagnostics: Vec<Diagnostic>,
    focus: FocusRing,
    /// This runtime's identity for the deferred binding write-backs (`UpdateSourceTrigger=LostFocus` / `Explicit`).
    binding_owner: u64,
    /// Turns the frame's input into the ordered WinForms event sequences (EVT-2); kept
    /// across reloads like the focus ring.
    router: InputRouter,
    /// The host this runtime paints in (see [`HostPort`]).
    port: Box<dyn HostPort>,
    /// The view's UI-thread services: dispatcher queue, async executor (EVT-6).
    ui: Rc<UiContext>,
    /// The non-visual `Timer` components of the view ([`Runtime::add_timer`]).
    timers: Vec<Timer>,
    /// A close asked for with [`Runtime::close`], handled by the next frame.
    pending_close: Option<CloseReason>,
    /// FormClosed ran: the view raises no event any more.
    closed: bool,
    form_closing: Event<FormClosingEventArgs>,
    form_closed: Event<FormClosedEventArgs>,
    /// What the elements asked of the window last frame (mnemonics, context menus, drop targets,
    /// the accessibility ids): what this frame's input is resolved against.
    last: crate::common::FrameServices,
    /// The tooltip under the pointer (`crate::window`).
    tooltip: crate::window::TooltipState,
    /// The open context menu.
    menu: Option<crate::window::OpenMenu>,
    /// Where the open menu's handlers run: `None` for the page's menus, the user control's scope for its own.
    menu_scope: Option<std::rc::Rc<dyn crate::events::router::DispatchScope>>,
    /// The check marks of menu items the user changed (`CheckOnClick`, `RadioGroup`), by item id.
    menu_checks: std::collections::HashMap<String, bool>,
    /// The menu bar in keyboard mode (Alt or F10), and its hot item (`crate::menus`).
    bar_focus: Option<(String, usize)>,
    /// Alt went down alone: its release puts the menu bar in keyboard mode.
    alt_armed: bool,
    /// The page control that had the focus when F6 moved it to the title band (F6 again gives it back).
    page_focus: Option<kubuno_ui::FocusId>,
    /// The menu bar whose `OnMenuActivate` was raised last.
    bar_active: Option<String>,
    /// The accessibility ids of the open menu's rows, and their paths.
    menu_access: Vec<(u64, Vec<usize>)>,
    /// The right button was down last frame (a context menu opens on its release).
    right_was_down: bool,
    left_was_down: bool,
    /// The element a drag is over (it got DragEnter), and the effect its handler answered.
    drag_target: Option<String>,
    drag_effect: crate::events::DragDropEffects,
    /// The folder of the view file (relative image paths): set by [`FileWatcher::poll`].
    base_dir: Option<PathBuf>,
    /// The ErrorProvider glyphs shown last frame: element id, message, shown since (ms).
    glyphs: Vec<(String, String, u64)>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // A window destroyed without a FormClosing (`host::quit`, an embedding parent gone)
        // still releases every worker blocked in `UiDispatcher::invoke`.
        self.shutdown();
    }
}

impl Runtime {
    /// A runtime for a view painted on the calling thread (the UI thread) by the Kubuno host.
    pub fn new() -> Self {
        let waker = host::ui_waker();
        Self::with_port(Box::new(RealHost), Box::new(move || waker.wake()))
    }

    /// A runtime for a view drawn INSIDE another view's window (an MDI document, an in-window
    /// dialog): it never closes, retitles or restyles the window it is drawn in, and the window's
    /// own close request is not its (close it with [`Runtime::close`]).
    pub fn new_inner() -> Self {
        let waker = host::ui_waker();
        Self::with_port(Box::new(InnerHost), Box::new(move || waker.wake()))
    }

    pub(crate) fn with_port(port: Box<dyn HostPort>, wake: Box<dyn Fn() + Send + Sync>) -> Self {
        // `{Res …}` images and live culture switches (vskubuno docs/RESOURCES.md).
        crate::resources::install();
        Self {
            compiled: None,
            diagnostics: Vec::new(),
            focus: FocusRing::new(),
            binding_owner: crate::binding::new_binding_owner(),
            router: InputRouter::new(),
            port,
            ui: UiContext::new(UiShared::new(wake)),
            timers: Vec::new(),
            pending_close: None,
            closed: false,
            form_closing: Event::new(),
            form_closed: Event::new(),
            last: crate::common::FrameServices::new(),
            tooltip: crate::window::TooltipState::default(),
            menu: None,
            menu_scope: None,
            menu_checks: std::collections::HashMap::new(),
            bar_focus: None,
            alt_armed: false,
            page_focus: None,
            bar_active: None,
            menu_access: Vec::new(),
            right_was_down: false,
            left_was_down: false,
            drag_target: None,
            drag_effect: crate::events::DragDropEffects::NONE,
            base_dir: None,
            glyphs: Vec::new(),
        }
    }

    /// The folder relative paths of the view (`BackgroundImage`, `Image`, `Icon`) are resolved
    /// against — [`FileWatcher::poll`] sets it to the view file's; takes effect at the next reload.
    pub fn set_base_dir(&mut self, dir: Option<PathBuf>) {
        crate::icon::set_default_base_dir(dir.as_deref());
        self.base_dir = dir;
    }

    /// The window's `Form` properties the view declares on its root element (`Title`, `Icon`,
    /// `StartPosition`, `FormBorderStyle`, the caption buttons, `ShowInTaskbar`, `TopMost`,
    /// `Opacity`, `WindowState`, `MinimumSize`/`MaximumSize`), with its bindings resolved against
    /// `vm` — what a window opening the view starts with (`HostOptions::form`); the runtime keeps
    /// the window in step every frame afterwards. `None` without a view.
    pub fn form_options(&self, vm: &dyn ViewModel) -> Option<host::FormOptions> {
        self.compiled.as_ref().map(|view| view.form.options(vm))
    }

    /// [`Runtime::form_options`] with the theme colours (`TitleBarBackground`, `BorderColor`…)
    /// resolved in `theme`.
    pub fn form_options_in(&self, vm: &dyn ViewModel, theme: &kubuno_ui::Theme) -> Option<host::FormOptions> {
        self.compiled.as_ref().map(|view| view.form.options_in(vm, Some(theme)))
    }

    /// The view's window properties as written (its `WindowKind`, `Chrome`, `IsMdiContainer`,
    /// `SplashDuration`…); `None` without a view.
    pub fn form_spec(&self) -> Option<&crate::window::FormSpec> {
        self.compiled.as_ref().map(|view| &view.form)
    }

    /// Raises the view's event for `event` at the next frame, as if the window had reported it.
    pub fn queue_window_event(&mut self, event: host::WindowEvent) {
        self.router.queue_window_events(vec![event]);
    }

    /// A handle that posts closures to this view's UI thread from any thread (WinForms
    /// `Control.Invoke`/`BeginInvoke`, see [`UiDispatcher`]). `V` is the view model type the view
    /// is painted with ([`Runtime::frame_typed`]); get it before handing it to a worker thread.
    ///
    /// ```no_run
    /// # use kubuno_views::runtime::Runtime;
    /// # struct MainViewModel { status: String }
    /// let runtime = Runtime::new();
    /// let dispatcher = runtime.dispatcher::<MainViewModel>();
    /// std::thread::spawn(move || loop {
    ///     std::thread::sleep(std::time::Duration::from_secs(1));
    ///     let posted = dispatcher.begin_invoke(|vm: &mut MainViewModel| vm.status = "tick".into());
    ///     if dispatcher.is_closed() { break; }
    ///     drop(posted);
    /// });
    /// ```
    pub fn dispatcher<V: 'static>(&self) -> UiDispatcher<V> {
        UiDispatcher::new(self.ui.shared.clone())
    }

    /// A [`UiHandle`] of this view (what async handlers receive), for async code started with
    /// [`Runtime::spawn_local`].
    pub fn ui_handle<V: 'static>(&self) -> UiHandle<V> {
        UiHandle::of(&self.ui)
    }

    /// Starts `future` on this view's UI-thread executor (see [`crate::events::spawn_local`],
    /// the form to use inside handlers). It is first polled at the next frame.
    pub fn spawn_local<T: 'static>(&self, future: impl std::future::Future<Output = T> + 'static) -> JoinHandle<T> {
        executor::spawn_on(&self.ui, future)
    }

    /// The view's named components (DATA-2, [`crate::scope`]): the instances the runtime created
    /// for its non-visual elements (`<BindingSource x:Name="customers">`…). Shared: it follows the
    /// hot reloads.
    pub fn components(&self) -> ComponentScope {
        self.ui.scope.clone()
    }

    /// Runs `f` on the component `name` of the view as a `T` —
    /// `runtime.with_component::<BindingSource, _>("customers", |bs| bs.move_next())`. `None` when
    /// there is none of that class. Its events reach the view's handlers at the next frame.
    pub fn with_component<T: crate::component::Component, R>(&self, name: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.ui.scope.with(name, f)
    }

    /// Adds a non-visual [`Timer`] to the view (WinForms' component tray): while it is enabled,
    /// its ticks run on the UI thread during this runtime's frames. Adding it twice is a no-op.
    pub fn add_timer(&mut self, timer: &Timer) {
        if !self.timers.iter().any(|t| t.same(timer)) {
            self.timers.push(timer.clone());
        }
    }

    /// Removes a timer added with [`Runtime::add_timer`].
    pub fn remove_timer(&mut self, timer: &Timer) {
        self.timers.retain(|t| !t.same(timer));
    }

    /// Asks the view to close with `reason`, like the window's close button but from code
    /// (WinForms `Form.Close`/`Application.Exit`): the next frame raises FormClosing (a handler
    /// may cancel it) and, if nobody cancelled, closes the window.
    pub fn close(&mut self, reason: CloseReason) {
        if !self.closed {
            self.pending_close = Some(reason);
            self.ui.shared.wake();
        }
    }

    /// Whether the view has closed (FormClosed ran, or [`Runtime::shutdown`] was called).
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// FormClosing for Rust subscribers (raised after the view's `OnFormClosing` handler; a
    /// subscriber may set `cancel` too).
    pub fn form_closing(&self) -> &Event<FormClosingEventArgs> {
        &self.form_closing
    }

    /// FormClosed for Rust subscribers.
    pub fn form_closed(&self) -> &Event<FormClosedEventArgs> {
        &self.form_closed
    }

    /// Stops the view's UI-thread services, in this order: async tasks cancelled (dropped at
    /// their `.await`), dispatcher closed (queued closures dropped, waiting `invoke`s released
    /// with `DispatchError::Closed`, new ones refused), timers stopped. Runs after FormClosed,
    /// and when the runtime is dropped. Idempotent.
    pub fn shutdown(&mut self) {
        self.closed = true;
        self.pending_close = None;
        self.ui.shutdown();
        for t in &self.timers {
            t.stop();
        }
    }

    /// Compiles `text` and, on success, replaces the current tree and clears
    /// the diagnostics. On failure, the diagnostics are replaced with the new
    /// ones but **the previous tree is left exactly as it was** — including
    /// "there was never a good tree yet", which leaves [`Runtime::has_view`]
    /// `false`. Returns whether the reload succeeded.
    pub fn reload_from_text(&mut self, text: &str) -> bool {
        match compile::compile_reusing(text, self.base_dir.as_deref(), self.ui.scope.reusable()) {
            Ok(mut view) => {
                self.ui.scope.replace(std::mem::take(&mut view.components));
                self.router.set_root(view.root_events.clone());
                self.router.set_key_preview(view.form.key_preview);
                self.menu = None;
                self.compiled = Some(view);
                self.diagnostics.clear();
                true
            }
            Err(diagnostics) => {
                self.diagnostics = diagnostics;
                false
            }
        }
    }

    /// The designer's reload (`vskubuno/docs/DESIGNER.md` §17, [`crate::tolerant`]): compiles `text`
    /// tolerantly, so a view with errors still shows everything that is valid (placeholders for the
    /// elements it cannot show, defaults for the values it cannot read). A text that is not even
    /// well-formed keeps the previous view on screen ([`DesignRenderState::Stale`]); with no previous
    /// view (a broken file opened), what the parser recovered is shown instead.
    pub fn reload_for_design(&mut self, text: &str) -> DesignReload {
        let had_view = self.compiled.is_some();
        let scope = self.ui.scope.clone();
        let outcome = crate::tolerant::compile_tolerant(text, self.base_dir.as_deref(), &|| scope.reusable(), !had_view);
        let state = match outcome.view {
            Some(mut view) => {
                self.ui.scope.replace(std::mem::take(&mut view.components));
                self.router.set_root(view.root_events.clone());
                self.router.set_key_preview(view.form.key_preview);
                self.menu = None;
                self.compiled = Some(view);
                if outcome.recovered {
                    DesignRenderState::Recovered
                } else if outcome.issues.is_empty() && outcome.diagnostics.is_empty() {
                    DesignRenderState::Clean
                } else {
                    DesignRenderState::Tolerant
                }
            }
            None if had_view => DesignRenderState::Stale,
            None => DesignRenderState::Empty,
        };
        self.diagnostics = outcome.diagnostics.clone();
        DesignReload { state, diagnostics: outcome.diagnostics, issues: if matches!(state, DesignRenderState::Stale) { Vec::new() } else { outcome.issues } }
    }

    /// The diagnostics of the last [`Runtime::reload_from_text`] call — empty
    /// after a successful one. What an in-app banner (see
    /// `examples/view_preview.rs`) shows while [`Runtime::has_view`] still
    /// paints the previous, good tree underneath.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether a tree has ever compiled successfully — `false` only before
    /// the first successful [`Runtime::reload_from_text`] (a broken file
    /// after at least one good load still leaves this `true`).
    pub fn has_view(&self) -> bool {
        self.compiled.is_some()
    }

    /// The size the current view was designed at (`DesignWidth`×`DesignHeight`, or its root's
    /// literal `Width`×`Height`), DIP - `None` without a view or when it declares none. A window
    /// hosting the view opens with this client size (like a WinForms `Form` at its designed `Size`)
    /// and paints the view over its whole client area: its `Anchor`ed children then follow every
    /// resize of the window.
    pub fn design_size(&self) -> Option<(f32, f32)> {
        self.compiled.as_ref().and_then(|view| view.design_size)
    }

    /// Paints one frame of the current tree (a no-op, painting nothing, when
    /// [`Runtime::has_view`] is `false`), returning the interactions it
    /// produced. Also dispatches any matching handler in `handlers` — see
    /// [`crate::node::PaintCx::fire`]. Equivalent to
    /// [`Runtime::frame_with_design`] with no layout map — every existing
    /// caller (`examples/view_preview.rs`, this module's own doc examples)
    /// keeps working unchanged; DSG-6's design mode is additive, not a
    /// breaking change to this method's signature.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        canvas: &dyn ControlCanvas,
        frame: &Frame,
        vm: &mut dyn ViewModel,
        handlers: &mut HandlerTable,
        bounds: Rect,
    ) -> Vec<ViewEvent> {
        self.frame_with_design(canvas, frame, vm, handlers, bounds, None)
    }

    /// [`Runtime::frame`] for a view model whose handlers are typed methods
    /// (`#[kubuno_views::event_handlers] impl V { … }`, `vskubuno/docs/EVENTS.md` §5.4): every
    /// event an element names a handler for runs `V`'s method of that name, with `&mut self`
    /// of the concrete type, a typed sender and the event's typed args.
    ///
    /// ```no_run
    /// # use kubuno_views::prelude::*;
    /// # use kubuno_views::runtime::Runtime;
    /// # #[derive(Default)] struct MainViewModel;
    /// # impl ViewModel for MainViewModel {
    /// #     fn get(&self, _: &str) -> Option<Value> { None }
    /// #     fn set(&mut self, _: &str, _: Value) {}
    /// # }
    /// #[kubuno_views::event_handlers]
    /// impl MainViewModel {
    ///     fn say_hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {}
    /// }
    /// # fn paint(runtime: &mut Runtime, canvas: &dyn kubuno_controls::ControlCanvas,
    /// #          frame: &kubuno_controls::host::Frame, vm: &mut MainViewModel, body: kubuno_ui::Rect) {
    /// let events = runtime.frame_typed(canvas, frame, vm, body);
    /// # }
    /// ```
    pub fn frame_typed<V: ViewModel + EventSink + 'static>(&mut self, canvas: &dyn ControlCanvas, frame: &Frame, vm: &mut V, bounds: Rect) -> Vec<ViewEvent> {
        let mut no_table = HandlerTable::new();
        self.frame_typed_with(canvas, frame, vm, &mut no_table, bounds)
    }

    /// [`Runtime::frame_typed`] with a legacy [`HandlerTable`] next to the typed handlers — the
    /// migration path (§5.4): an event runs `V`'s typed method when it has one of that name,
    /// else the table's entry (a legacy `handlers!` entry receives the event's legacy value, as
    /// with [`Runtime::frame`]).
    pub fn frame_typed_with<V: ViewModel + EventSink + 'static>(
        &mut self,
        canvas: &dyn ControlCanvas,
        frame: &Frame,
        vm: &mut V,
        handlers: &mut HandlerTable,
        bounds: Rect,
    ) -> Vec<ViewEvent> {
        self.run_frame(Some(canvas), frame, &mut TypedVm(vm), handlers, bounds, None)
    }

    /// [`Runtime::frame`] for a view model that dispatches its events itself: every event an
    /// element names a handler for goes to `vm`'s [`ViewModel::dispatch_event`] (no table, no
    /// [`EventSink`]), and the closures of [`Runtime::dispatcher`] / the tasks of
    /// [`Runtime::spawn_local`] get `&mut V`, as with [`Runtime::frame_typed`]. What the `kubuno`
    /// crate's forms paint with: their view model routes each event to the form's handler methods
    /// and to the Rust subscribers of its controls.
    pub fn frame_model<V: ViewModel + 'static>(&mut self, canvas: &dyn ControlCanvas, frame: &Frame, vm: &mut V, bounds: Rect) -> Vec<ViewEvent> {
        let mut no_table = HandlerTable::new();
        self.run_frame(Some(canvas), frame, &mut ModelVm(vm), &mut no_table, bounds, None)
    }

    /// [`Runtime::frame`], with an optional [`crate::design::LayoutMap`] to
    /// record into (`vskubuno/docs/DESIGNER.md` §6, DSG-6) — `None` behaves
    /// identically to [`Runtime::frame`] (no recording, no added cost beyond
    /// the one branch `crate::design::DesignSlot::paint` already documents).
    /// The caller (`examples/view_embed.rs`) is expected to
    /// [`crate::design::LayoutMap::clear`] the map before each call that
    /// passes `Some` — this method does not clear it itself, so a caller
    /// that wants to ACCUMULATE across frames (nothing in this crate does)
    /// remains free to.
    #[allow(clippy::too_many_arguments)]
    pub fn frame_with_design(
        &mut self,
        canvas: &dyn ControlCanvas,
        frame: &Frame,
        vm: &mut dyn ViewModel,
        handlers: &mut HandlerTable,
        bounds: Rect,
        design: Option<&mut crate::design::LayoutMap>,
    ) -> Vec<ViewEvent> {
        self.run_frame(Some(canvas), frame, &mut DynVm(vm), handlers, bounds, design)
    }

    /// One frame (see the module doc for the order). `canvas` is `None` only in this crate's
    /// tests, which run everything but the paint.
    fn run_frame<F: FrameVm>(
        &mut self,
        canvas: Option<&dyn ControlCanvas>,
        frame: &Frame,
        vm: &mut F,
        handlers: &mut HandlerTable,
        bounds: Rect,
        design: Option<&mut crate::design::LayoutMap>,
    ) -> Vec<ViewEvent> {
        // Bound icon files are resolved against the view's folder while it paints.
        let _icons = crate::icon::enter_base_dir(self.base_dir.as_deref());
        // An open context menu takes the pointer while it is over it: the view under it sees the
        // pointer away (no hover, no click through the menu).
        let real_frame = frame;
        let masked;
        // So does an open `<Popover>` (all of it while a light-dismiss one is open: the press outside
        // it only closes it), which reads the real frame (`crate::node::TopLayer`).
        let held = design.is_none() && crate::node::top_layer_holds(frame.mouse.0, frame.mouse.1);
        crate::node::set_real_frame(held.then_some(*frame));
        let frame: &Frame = if held || (design.is_none() && self.menu.as_ref().is_some_and(|m| m.contains(frame.mouse.0, frame.mouse.1))) {
            masked = crate::common::away_frame(frame);
            &masked
        } else {
            frame
        };
        self.focus.begin_frame(frame);
        let mut events = Vec::new();
        // Taken out for the frame so the rest of `self` stays freely usable; put back below.
        let Some(mut view) = self.compiled.take() else {
            self.focus.end_frame();
            return events;
        };
        // The designer (a layout map is recorded) never raises the application's events, and a
        // closed view raises none any more.
        let mut routed = design.is_none() && !self.closed;
        // The binding paths that name a component go to it (DATA-2).
        let scope = self.ui.scope.clone();
        let mut scoped = ScopedFrame { inner: vm, scope: scope.clone(), live: routed };
        let vm = &mut scoped;
        let _binding_owner = crate::binding::enter_binding_owner(self.binding_owner);
        let _current = executor::enter(&self.ui);
        let mut activate = Vec::new();
        if routed {
            let now = self.port.now_ms();
            self.ui.set_now(now);
            let mut input_events = self.port.events();
            if self.handle_close_request(&input_events, vm, handlers, &mut events) {
                routed = false;
            } else {
                // The window's own input first: the open menu, mnemonics, assistive technology,
                // dropped files, a right click opening a context menu (`crate::window`).
                vm.with_vm(|vm| {
                    let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut events };
                    self.window_input(&input_events, real_frame, &view, &mut d, &mut activate);
                });
                input_events = self.port.events();
                self.router.queue_window_events(self.port.window_events());
                let input = FrameInput { frame, now_ms: now, events: &input_events };
                let focus_before = (self.focus.focused(), self.focus.window_active());
                let outcome = vm.with_vm(|vm| {
                    let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut events };
                    self.router.begin_frame(&input, &mut self.focus, &mut d)
                });
                // The focus moved (or left the window): `UpdateSourceTrigger=LostFocus` bindings write now.
                if (self.focus.focused(), self.focus.window_active()) != focus_before {
                    vm.with_vm(|vm| {
                        crate::binding::commit_lost_focus(vm);
                    });
                }
                for i in outcome.consumed {
                    if let Some(target) = input_events.get(i) {
                        self.port.consume(target);
                    }
                }
                if let Some(ms) = outcome.repaint_after {
                    self.port.request_repaint_after(ms);
                }
                // Work posted for the UI thread: closures, timer ticks, woken tasks.
                vm.run_jobs(&self.ui);
                let now = self.ui.now();
                let timers = self.timers.clone();
                let next_tick = vm.with_vm(|vm| {
                    let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut events };
                    timers.iter().filter_map(|t| t.pump(now, &mut d)).min()
                });
                self.ui.fire_timers();
                vm.run_tasks(&self.ui);
                // The named components follow each other (a detail list its master), and the events
                // they queued while code changed them reach their handlers (DATA-2).
                vm.with_vm(|vm| {
                    if scope.sync_all(&mut *vm, true) {
                        self.port.request_repaint_after(1);
                    }
                    scope.deliver(&mut *handlers, vm, &mut events);
                });
                if let Some(due) = next_tick {
                    self.port.request_wake_after(u32::try_from(due.saturating_sub(now)).unwrap_or(u32::MAX).max(1));
                }
            }
        }
        // What the menus show this frame (`crate::menus`): the open bar menu, the bar in keyboard mode,
        // the open drop-down; in the designer, where a context menu being designed shows.
        crate::menus::set_open_bar(if routed { self.menu.as_ref().and_then(|m| m.bar.clone()) } else { None });
        crate::menus::set_bar_focus(if routed { self.bar_focus.clone() } else { None });
        crate::window::set_open_menu(if routed { self.menu.as_ref().map(|m| m.spec.name.clone()) } else { None });
        crate::menus::set_design_view(design.is_some().then_some(bounds));
        let mut services = crate::common::FrameServices::new();
        services.activate = activate;
        if let Some(canvas) = canvas {
            let router = if routed { Some(&mut self.router) } else { None };
            let focus = &mut self.focus;
            crate::common::set_design_frame(design.is_some());
            vm.with_vm(|vm| {
                let mut cx = PaintCx {
                    canvas,
                    frame,
                    vm,
                    focus,
                    handlers: &mut *handlers,
                    events: &mut events,
                    design,
                    router,
                    sender: None,
                    control: None,
                    services: Some(&mut services),
                    activate: false,
                };
                view.root.paint(&mut cx, bounds);
                // What a node drew over its siblings in the designer (a ribbon's drop-down).
                crate::virtual_regions::flush_late(cx.canvas, cx.design.as_deref_mut());
                // The menu rows shown on the design surface: where a dragged row may go next frame.
                if cx.design.is_some() {
                    crate::menus::finish_design_rows();
                }
            });
            crate::common::set_design_frame(false);
        }
        if routed {
            // The ErrorProvider glyphs next to the controls bound to a field in error (DATA-2).
            if let Some(canvas) = canvas {
                self.paint_error_glyphs(canvas, frame, &scope, &mut services);
            }
            vm.with_vm(|vm| {
                // Events the components queued during the paint (a handler changed them).
                if scope.deliver(&mut *handlers, &mut *vm, &mut events) > 0 {
                    self.port.request_repaint_after(1);
                }
                let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut events };
                self.router.set_window_visible(self.port.window_visible());
                self.router.end_frame(&mut d);
                // What the elements asked of the window: default buttons, pointer shape, tooltip,
                // accessibility tree, `Form` properties, dropped files, the open menu.
                self.window_output(canvas, real_frame, &view, &services, &mut d);
            });
            self.last = services;
        }
        self.compiled = Some(view);
        if routed {
            // Tasks this frame's handlers started (async handlers) get their first poll now;
            // what they changed shows at the next frame.
            if vm.run_tasks(&self.ui) > 0 {
                self.port.request_repaint_after(1);
            }
            if let Some(due) = self.ui.next_deadline() {
                let now = self.ui.now();
                self.port.request_wake_after(u32::try_from(due.saturating_sub(now)).unwrap_or(u32::MAX).max(1));
            }
            // Closing goes through this view's FormClosing from now on (`handle_close_request`).
            self.port.defer_close();
        }
        self.focus.end_frame();
        events
    }

    /// Step 1 of a frame: a close request (the host's, or [`Runtime::close`]'s) raises
    /// FormClosing; unless a handler cancelled it, FormClosed → Deactivate, then the view shuts
    /// down and the host loop is asked to end. Returns whether the view closed.
    fn handle_close_request<F: FrameVm>(&mut self, input_events: &[InputEvent], vm: &mut F, handlers: &mut HandlerTable, events: &mut Vec<ViewEvent>) -> bool {
        let request = input_events.iter().find(|e| matches!(e, InputEvent::CloseRequested(_)));
        let reason = match request {
            Some(InputEvent::CloseRequested(r)) => close_reason(*r),
            _ => match self.pending_close.take() {
                Some(r) => r,
                None => return false,
            },
        };
        if let Some(e) = request {
            self.port.consume(e);
        }
        let mut args = vm.with_vm(|vm| {
            let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut *events };
            self.router.form_closing(&mut d, reason)
        });
        let root = self.router.root_sender();
        if self.form_closing.has_subscribers() {
            let sender = match &root {
                Some((s, b)) => s.sender(*b),
                None => crate::events::ElementRef::detached("view"),
            };
            self.form_closing.raise(&sender, &mut args);
        }
        if args.cancel {
            tracing::debug!("close ({reason:?}) cancelled by FormClosing");
            self.port.cancel_close();
            return false;
        }
        vm.with_vm(|vm| {
            let mut d = Dispatch { vm, handlers: &mut *handlers, events: &mut *events };
            self.router.form_closed(&mut d, reason);
        });
        if self.form_closed.has_subscribers() {
            let sender = match &root {
                Some((s, b)) => s.sender(*b),
                None => crate::events::ElementRef::detached("view"),
            };
            self.form_closed.raise(&sender, &mut FormClosedEventArgs { reason });
        }
        self.shutdown();
        self.port.quit();
        true
    }
}

/// The window services of a frame (`crate::common`, `crate::window`): what the view asks of its
/// window around the input router and the paint.
impl Runtime {
    /// Before the router: the open menu takes the keyboard and its clicks (a menu bar's menus follow
    /// the pointer and Left/Right from one to the next); a menu bar in keyboard mode (Alt or F10) takes
    /// its keys; the shortcuts of the menus and commands run (WinForms' `ProcessCmdKey`); Alt + a
    /// mnemonic letter activates its element (or moves the focus after a label); assistive
    /// technology's clicks and focus moves; dropped files raise DragDrop; a right-button release over
    /// an element with a `ContextMenu` opens it. Every event used is consumed.
    fn window_input(&mut self, input: &[InputEvent], frame: &Frame, view: &CompiledView, d: &mut Dispatch<'_>, activate: &mut Vec<String>) {
        use kubuno_controls::host::vk;
        let now = self.port.now_ms();
        let closing_before = self.menu.as_ref().map(closed_handler);
        let mut access_actions = self.port.take_access_actions();
        let pressed = (frame.mouse_down && !self.left_was_down) || (frame.right_down && !self.right_was_down);

        // The open menu.
        let menu_was_open = self.menu.is_some();
        if let Some(mut menu) = self.menu.take() {
            use crate::window::MenuAction;
            let mut keys = Vec::new();
            let mut letters = Vec::new();
            for e in input {
                match e {
                    InputEvent::Key { vk: k, down: true, mods, .. } => {
                        // Alt+F4 still closes the window.
                        if mods.alt && *k == vk::F4 {
                            continue;
                        }
                        if !mods.alt && [vk::UP, vk::DOWN, vk::HOME, vk::END, vk::ENTER, vk::SPACE, vk::ESCAPE, vk::LEFT, vk::RIGHT].contains(k) {
                            keys.push(*k);
                        } else if !mods.ctrl && !mods.alt {
                            if let Some(c) = char::from_u32(u32::from(*k)).filter(|c| c.is_ascii_alphanumeric()) {
                                letters.push(c);
                            }
                        } else if *k == vk::MENU {
                            // Alt pressed while a menu is open closes it.
                            keys.push(vk::ESCAPE);
                        }
                        // The menu has the keyboard: nothing reaches the focused control.
                        self.port.consume(e);
                    }
                    InputEvent::Text(_) => self.port.consume(e),
                    _ => {}
                }
            }
            let mut action = menu.keys(&keys);
            if action == MenuAction::None {
                for c in letters {
                    action = menu.mnemonic(c);
                    if action != MenuAction::None {
                        menu.keyboard = true;
                        break;
                    }
                }
            }
            // Assistive technology pressed one of its rows (published by `window_output`).
            access_actions.retain(|(id, _)| match self.menu_access.iter().find(|(a, _)| a == id) {
                Some((_, path)) => {
                    if action == MenuAction::None {
                        action = menu.access_click(path);
                    }
                    false
                }
                None => true,
            });
            let (x, y) = frame.mouse;
            let mut opened: Option<Vec<usize>> = menu.hover(x, y);
            if let MenuAction::Opened(path) = action {
                opened = Some(path);
                action = MenuAction::None;
            }
            // A menu bar's menu: the pointer over another label of the bar opens that one.
            let bar = menu.bar.clone();
            let mut switch_to: Option<(usize, bool)> = None;
            if let Some((bar_id, index)) = &bar {
                if let Some(info) = self.last.menu_bars.iter().find(|b| b.id == *bar_id) {
                    if !frame.pointer_outside() {
                        if let Some(j) = info.items.iter().position(|it| it.rect.contains(x, y)) {
                            if j != *index && info.items[j].enabled {
                                switch_to = Some((j, false));
                            } else if j == *index && pressed {
                                // A click on the open menu's own label closes it (and must not reopen it).
                                crate::menus::suppress_bar_press(bar_id, j);
                                action = MenuAction::Close;
                            }
                        }
                    }
                    if let MenuAction::Neighbour(next) = action {
                        switch_to = next_enabled(&info.items, *index, next).map(|j| (j, true));
                        action = MenuAction::None;
                    }
                }
            }
            if pressed && action == MenuAction::None && switch_to.is_none() {
                action = menu.click(x, y).unwrap_or(MenuAction::Close);
            }
            if frame.dismiss {
                action = MenuAction::Close;
            }
            if let MenuAction::Opened(path) = action {
                opened = Some(path);
                action = MenuAction::None;
            }
            if let Some(path) = opened {
                // `DropDownOpening`: the handler may fill the sub-menu (its `ItemsSource`), read now.
                let checks = &self.menu_checks;
                scoped(d, self.menu_scope.as_ref(), &mut |d| open_submenu(d, &mut menu, &path, checks));
                self.port.request_repaint_after(1);
            }
            // The command the pointer or the keyboard rests on (its tooltip shows after a while).
            let hot = menu.hot_path();
            if menu.rest.as_ref().map(|(p, _)| Some(p)) != Some(hot.as_ref()) {
                menu.rest = hot.map(|p| (p, now));
            }
            match (action, switch_to) {
                (_, Some((j, keyboard))) => {
                    let keyboard = keyboard || menu.keyboard;
                    if let (Some((bar_id, _)), Some(info)) = (&bar, bar.as_ref().and_then(|(b, _)| self.last.menu_bars.iter().find(|i| i.id == *b))) {
                        let item = info.items[j].clone();
                        let mut request = crate::window::MenuRequest::new(item.menu, crate::window::MenuAnchor::Below(item.rect));
                        request.bar = Some((bar_id.clone(), j));
                        request.keyboard = keyboard;
                        self.open_menu(view, d, frame, request);
                    }
                    if self.menu.is_none() {
                        self.menu = Some(menu);
                    }
                    self.port.request_repaint_after(1);
                }
                (MenuAction::Chosen(path), None) => {
                    let spec = menu.spec.clone();
                    if let Some(item) = spec.item_at(&path) {
                        let siblings = path.split_last().and_then(|(_, parent)| spec.items_at(parent)).map(<[_]>::to_vec).unwrap_or_default();
                        let checks = &mut self.menu_checks;
                        let commands = &view.commands;
                        scoped(d, self.menu_scope.as_ref(), &mut |d| choose_menu_item(d, &spec, item, &siblings, checks, frame, commands));
                    }
                    self.port.request_repaint_after(1);
                }
                (MenuAction::Close, None) => {
                    // Escape on a menu bar's menu opened from the keyboard: the bar keeps the keyboard.
                    if let (Some((bar_id, index)), true) = (&bar, menu.keyboard && keys.contains(&vk::ESCAPE)) {
                        self.bar_focus = Some((bar_id.clone(), *index));
                    }
                    self.port.request_repaint_after(1);
                }
                _ => self.menu = Some(menu),
            }
        }

        // A menu bar in keyboard mode (Alt or F10 pressed alone), or put in it now.
        // (Not in the frame the open menu used the keys: Escape closing a menu must not also leave the bar.)
        if self.menu.is_none() && !menu_was_open {
            self.bar_keys(input, frame, view, d, pressed);
        } else if self.menu.is_some() {
            self.alt_armed = false;
            self.bar_focus = None;
        }

        // The shortcuts of the menus and commands, before the focused control reads its keys.
        if self.menu.is_none() && !view.accelerators.is_empty() {
            for e in input {
                let InputEvent::Key { vk: k, down: true, mods, .. } = e else { continue };
                let Some(accel) = view.accelerators.iter().find(|a| crate::menus::matches(&a.shortcut, *k, *mods)).cloned() else { continue };
                if self.run_accelerator(view, d, frame, &accel.target) {
                    self.port.consume(e);
                    self.port.request_repaint_after(1);
                }
            }
        }
        // A split button's `DefaultItem` asked to run.
        for (menu, item) in crate::menus::take_run_items() {
            self.run_accelerator(view, d, frame, &crate::menus::AcceleratorTarget::Item { menu, item });
        }

        // Mnemonics: Alt + the letter of a `&Save`.
        for e in input {
            let InputEvent::Key { vk: k, down: true, mods, repeat: false } = e else { continue };
            if !mods.alt || mods.ctrl {
                continue;
            }
            let Some(key) = char::from_u32(u32::from(*k)).filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()) else { continue };
            let Some(m) = self.last.mnemonics.iter().find(|m| m.key == key).cloned() else { continue };
            self.port.consume(e);
            match m.action {
                crate::common::MnemonicAction::Activate => activate.push(m.element),
                crate::common::MnemonicAction::FocusNext => {
                    if let Some(id) = self.router.focus_after(&m.element) {
                        self.focus.focus_visibly(id);
                    }
                }
            }
        }

        // F6 / Shift+F6: the focus from the page to the title band's controls (`TitleBar.Region`, the standard header
        // items) and back, like the panes of a Windows application.
        for e in input {
            let InputEvent::Key { vk: k, down: true, mods, .. } = e else { continue };
            if *k == vk::F6 && !mods.ctrl && !mods.alt && self.toggle_band_focus() {
                self.port.consume(e);
            }
        }

        // Assistive technology (Narrator, NVDA…): press an element, move the focus.
        for (id, action) in access_actions {
            let Some((_, element, focus_id)) = self.last.access_ids.iter().find(|(a, ..)| *a == id).cloned() else { continue };
            match action {
                host::access::AccessAction::Click => activate.push(element),
                host::access::AccessAction::Focus => {
                    if let Some(f) = focus_id {
                        self.focus.focus_visibly(f);
                    }
                }
            }
        }

        // Drag and drop (EVT-8): the drag over the window goes to the deepest element that allows
        // drops under the pointer (last frame's geometry).
        if let Some(drag) = self.port.drag() {
            self.route_drag(d, &drag);
        } else if let Some(old) = self.drag_target.take() {
            // The drag vanished without a leave (the window was not rendered for it).
            self.router.raise_on(d, &old, "OnDragLeave", crate::events::EmptyEventArgs);
            self.drag_effect = crate::events::DragDropEffects::NONE;
        }

        // Files dropped through the shell's own path (no OLE drop target on the window).
        for e in input {
            let InputEvent::FilesDropped { x, y, files } = e else { continue };
            self.port.consume(e);
            let (x, y) = (*x as f32, *y as f32);
            let Some((target, bounds)) = self.last.drop_targets.iter().rev().find(|(_, b)| b.contains(x, y)).cloned() else { continue };
            let args = crate::events::DragEventArgs {
                data: crate::events::DataObject { files: files.iter().map(PathBuf::from).collect(), ..Default::default() },
                allowed: crate::events::DragDropEffects::COPY,
                effect: crate::events::DragDropEffects::COPY,
                x: x - bounds.left,
                y: y - bounds.top,
                mods: frame.mods,
                key_state: 0,
            };
            self.router.raise_on(d, &target, "OnDragDrop", args);
        }

        // A right-button release over an element with a context menu opens it at the pointer.
        let right_released = !frame.right_down && self.right_was_down;
        if right_released && self.menu.is_none() && !frame.pointer_outside() {
            let (x, y) = frame.mouse;
            let target = self.last.context_menus.iter().rev().find(|(_, b, _)| b.contains(x, y)).cloned();
            if let Some((owner, _, name)) = target {
                let mut request = crate::window::MenuRequest::new(name, crate::window::MenuAnchor::Point(x, y));
                request.owner = owner;
                self.open_menu(view, d, frame, request);
            }
        }
        // A menu opened from code (`show_context_menu`, a drop-down button, a menu bar).
        if self.menu.is_none() {
            let names: Vec<&str> = view.menus.iter().map(|m| m.name.as_str()).chain(self.last.local_menus.iter().map(|m| m.name.as_str())).collect();
            if let Some(request) = crate::window::take_menu_request(&names) {
                self.open_menu(view, d, frame, request);
            }
        }
        if self.menu.is_some() {
            self.tooltip.press(now);
        }

        // A menu that closed (or gave way to another one): its `OnClosed`.
        let closing_after = self.menu.as_ref().map(closed_handler);
        if let Some(before) = closing_before.filter(|b| Some(b) != closing_after.as_ref()) {
            if let Some(handler) = &before.handler {
                let sender = ElementRef { name: before.name.as_deref(), element: before.element, id: &before.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
                let scope = self.menu_scope.clone();
                scoped(d, scope.as_ref(), &mut |d| {
                    d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut crate::events::EmptyEventArgs);
                });
            }
        }
        // A menu bar takes or gives back the keyboard: its `OnMenuActivate` / `OnMenuDeactivate`.
        let active = self.menu.as_ref().and_then(|m| m.bar.as_ref().map(|(b, _)| b.clone())).or_else(|| self.bar_focus.as_ref().map(|(b, _)| b.clone()));
        if active != self.bar_active {
            if let Some(old) = self.bar_active.take() {
                self.router.raise_on(d, &old, "OnMenuDeactivate", crate::events::EmptyEventArgs);
            }
            if let Some(new) = &active {
                self.router.raise_on(d, new, "OnMenuActivate", crate::events::EmptyEventArgs);
            }
            self.bar_active = active;
        }
        self.right_was_down = frame.right_down;
        self.left_was_down = frame.mouse_down;
    }

    /// Opens a menu of the view (`request`): its `OnOpening` (or its owner's `OnDropDownOpening`) runs,
    /// then the menu is read (bindings, list items) and shown where the request says.
    fn open_menu(&mut self, view: &CompiledView, d: &mut Dispatch<'_>, frame: &Frame, request: crate::window::MenuRequest) {
        let Some((spec, scope)) = find_menu(view, &self.last, &request.name) else { return };
        let (dx, dy) = host::content_offset();
        // A menu of a menu bar opens under its label, wherever the bar was painted last.
        let mut anchor = request.anchor;
        if let Some((bar, index)) = &request.bar {
            if let Some(item) = self.last.menu_bars.iter().find(|b| b.id == *bar).and_then(|b| b.items.get(*index)) {
                anchor = crate::window::MenuAnchor::Below(item.rect);
            }
        }
        let (at, owner) = match anchor {
            crate::window::MenuAnchor::Point(x, y) => ((x, y), request.owner),
            crate::window::MenuAnchor::Below(r) => ((r.left, r.bottom + 2.0), request.owner),
            crate::window::MenuAnchor::Element(e) => match self.router.bounds_of(&e) {
                Some(b) => ((b.left + dx, b.bottom + dy + 4.0), e),
                None => (frame.mouse, e),
            },
        };
        let checks = &self.menu_checks;
        let mut resolved = None;
        scoped(d, scope.as_ref(), &mut |d| {
            if let Some(handler) = &spec.opening {
                let (element, name) = spec.sender_element();
                let sender = ElementRef { name, element, id: &spec.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
                d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut crate::events::CancelEventArgs::default());
            }
            resolved = Some(spec.resolved(&*d.vm, checks));
        });
        self.menu_scope = scope;
        let mut menu = crate::window::OpenMenu::new(spec.clone(), resolved.unwrap_or_else(|| spec.clone()), at, owner);
        menu.keyboard = request.keyboard;
        menu.bar = request.bar;
        if menu.keyboard {
            menu.hot_first();
        }
        self.menu = Some(menu);
        self.bar_focus = None;
        self.port.request_repaint_after(1);
    }

    /// F6 (or Alt alone without a menu bar): the focus goes to the first focusable control of the window's title
    /// band (last frame's), remembering the page's; from the band, it goes back to that control (or leaves the
    /// band). `false` when the band has no focusable control (the key is then left to the page).
    fn toggle_band_focus(&mut self) -> bool {
        let band = &self.last.title_band_focus;
        let Some(first) = band.first().copied() else { return false };
        match self.focus.focused() {
            Some(current) if band.contains(&current) => match self.page_focus.take() {
                Some(page) => self.focus.focus_visibly(page),
                None => self.focus.blur(),
            },
            current => {
                self.page_focus = current;
                self.focus.focus_visibly(first);
            }
        }
        self.port.request_repaint_after(1);
        true
    }

    /// The keys of a menu bar in keyboard mode: Alt or F10 alone enters (or leaves) it; Left and
    /// Right move between its labels; Down, Up, Enter and Space open the hot one's menu, as does its
    /// mnemonic letter; Escape, a click or any other key leaves it.
    fn bar_keys(&mut self, input: &[InputEvent], frame: &Frame, view: &CompiledView, d: &mut Dispatch<'_>, pressed: bool) {
        use kubuno_controls::host::vk;
        if pressed {
            self.alt_armed = false;
            self.bar_focus = None;
        }
        let bars = &self.last.menu_bars;
        let mut open: Option<(String, usize)> = None;
        // Alt released alone in a view without a menu bar (the title band takes the focus after the loop).
        let mut band_key: Option<&InputEvent> = None;
        for e in input {
            let InputEvent::Key { vk: k, down, repeat, mods } = e else { continue };
            // Alt pressed and released alone, or F10: the first bar of the view takes the keyboard.
            if *k == vk::MENU {
                if *down && !*repeat {
                    self.alt_armed = !mods.ctrl && !mods.shift;
                } else if !*down && std::mem::take(&mut self.alt_armed) {
                    // A view without a menu bar: Alt alone reaches the title band's controls (as F6 does).
                    if bars.is_empty() {
                        band_key = Some(e);
                        continue;
                    }
                    self.bar_focus = match self.bar_focus.take() {
                        Some(_) => None,
                        None => bars.first().and_then(|b| next_enabled(&b.items, b.items.len().wrapping_sub(1), true).map(|i| (b.id.clone(), i))),
                    };
                    if self.bar_focus.is_some() || !bars.is_empty() {
                        self.port.consume(e);
                    }
                }
                continue;
            }
            if !*down {
                continue;
            }
            self.alt_armed = false;
            if *k == vk::F10 && mods.is_none() {
                if let Some(b) = bars.first() {
                    self.bar_focus = match self.bar_focus.take() {
                        Some(_) => None,
                        None => next_enabled(&b.items, b.items.len().wrapping_sub(1), true).map(|i| (b.id.clone(), i)),
                    };
                    self.port.consume(e);
                }
                continue;
            }
            let Some((bar_id, index)) = self.bar_focus.clone() else { continue };
            let Some(bar) = bars.iter().find(|b| b.id == bar_id) else {
                self.bar_focus = None;
                continue;
            };
            self.port.consume(e);
            match *k {
                k if k == vk::LEFT || k == vk::RIGHT => {
                    if let Some(j) = next_enabled(&bar.items, index, k == vk::RIGHT) {
                        self.bar_focus = Some((bar_id, j));
                    }
                }
                k if k == vk::DOWN || k == vk::UP || k == vk::ENTER || k == vk::SPACE => open = Some((bar_id, index)),
                k if k == vk::ESCAPE => self.bar_focus = None,
                k => {
                    let letter = char::from_u32(u32::from(k)).filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase());
                    match letter.and_then(|c| bar.items.iter().position(|it| it.enabled && it.key == Some(c))) {
                        Some(j) => open = Some((bar_id, j)),
                        // Any other key leaves the bar (and reaches the focused control).
                        None => self.bar_focus = None,
                    }
                }
            }
            if open.is_some() {
                break;
            }
        }
        if let Some(e) = band_key {
            if self.toggle_band_focus() {
                self.port.consume(e);
            }
        }
        // The text of a letter that opened a menu or was eaten by the bar.
        if self.bar_focus.is_some() || open.is_some() {
            for e in input {
                if matches!(e, InputEvent::Text(_)) {
                    self.port.consume(e);
                }
            }
        }
        if let Some((bar_id, j)) = open {
            if let Some(item) = self.last.menu_bars.iter().find(|b| b.id == bar_id).and_then(|b| b.items.get(j)).cloned() {
                let mut request = crate::window::MenuRequest::new(item.menu, crate::window::MenuAnchor::Below(item.rect));
                request.bar = Some((bar_id, j));
                request.keyboard = true;
                self.open_menu(view, d, frame, request);
            }
        }
    }

    /// Runs what a shortcut names: a menu item (enabled and visible as its menu would show it), or a
    /// command. Returns whether something ran.
    fn run_accelerator(&mut self, view: &CompiledView, d: &mut Dispatch<'_>, frame: &Frame, target: &crate::menus::AcceleratorTarget) -> bool {
        match target {
            crate::menus::AcceleratorTarget::Item { menu, item } => {
                let Some((spec, scope)) = find_menu(view, &self.last, menu) else { return false };
                let mut ran = false;
                let checks = &mut self.menu_checks;
                let commands = &view.commands;
                scoped(d, scope.as_ref(), &mut |d| {
                    let resolved = spec.resolved(&*d.vm, checks);
                    let Some(path) = path_of(&resolved.items, item) else { return };
                    let Some(found) = resolved.item_at(&path).cloned() else { return };
                    if !found.enabled || !found.is_command() {
                        return;
                    }
                    let siblings = path.split_last().and_then(|(_, parent)| resolved.items_at(parent)).map(<[_]>::to_vec).unwrap_or_default();
                    choose_menu_item(d, &resolved, &found, &siblings, checks, frame, commands);
                    ran = true;
                });
                ran
            }
            crate::menus::AcceleratorTarget::Command(name) => {
                let Some(cmd) = view.commands.iter().find(|c| c.name == *name).cloned() else { return false };
                let resolved = cmd.resolved(&*d.vm, &self.menu_checks);
                if !resolved.enabled {
                    return false;
                }
                execute_command(d, &cmd, &mut self.menu_checks);
                true
            }
        }
    }

    /// Routes the drag over the window (WinForms' drop-target sequence): the element under the
    /// pointer that allows drops gets `DragEnter`, then `DragOver` while the drag moves over it,
    /// `DragLeave` when the drag moves off it or is cancelled, and `DragDrop` when the data is
    /// dropped on it with an effect its handlers accepted. The effect a handler answers (within what
    /// the source allows) is kept from one event to the next and returned to the source.
    fn route_drag(&mut self, d: &mut Dispatch<'_>, drag: &host::dnd::DragFrame) {
        use crate::events::{DragDropEffects, DragEventArgs, EmptyEventArgs};
        use host::dnd::DragPhase;
        let target = self.last.drop_targets.iter().rev().find(|(_, b)| b.contains(drag.x, drag.y)).cloned();
        let key_state = u32::from(drag.buttons.0)
            | u32::from(drag.buttons.1) << 1
            | u32::from(drag.mods.shift) << 2
            | u32::from(drag.mods.ctrl) << 3
            | u32::from(drag.buttons.2) << 4
            | u32::from(drag.mods.alt) << 5;
        let args = |bounds: Rect, effect: DragDropEffects| DragEventArgs {
            data: (*drag.data).clone(),
            allowed: drag.allowed,
            effect,
            x: drag.x - bounds.left,
            y: drag.y - bounds.top,
            mods: drag.mods,
            key_state,
        };
        let same = target.as_ref().map(|(id, _)| id) == self.drag_target.as_ref();
        if !same || drag.phase == DragPhase::Leave {
            if let Some(old) = self.drag_target.take() {
                self.router.raise_on(d, &old, "OnDragLeave", EmptyEventArgs);
            }
            self.drag_effect = DragDropEffects::NONE;
        }
        let mut effect = DragDropEffects::NONE;
        match (drag.phase, target) {
            (DragPhase::Enter | DragPhase::Over, Some((id, bounds))) => {
                let event = if self.drag_target.is_none() { "OnDragEnter" } else { "OnDragOver" };
                if let Some(a) = self.router.raise_on(d, &id, event, args(bounds, self.drag_effect)) {
                    effect = a.effect & drag.allowed;
                }
                self.drag_target = Some(id);
                self.drag_effect = effect;
            }
            (DragPhase::Drop, Some((id, bounds))) => {
                effect = self.drag_effect;
                if self.drag_target.is_none() {
                    if let Some(a) = self.router.raise_on(d, &id, "OnDragEnter", args(bounds, DragDropEffects::NONE)) {
                        effect = a.effect & drag.allowed;
                    }
                }
                // WinForms raises DragDrop only when the target accepted the drag.
                if !effect.is_none() {
                    if let Some(a) = self.router.raise_on(d, &id, "OnDragDrop", args(bounds, effect)) {
                        effect = a.effect & drag.allowed;
                    }
                }
                self.drag_target = None;
                self.drag_effect = DragDropEffects::NONE;
            }
            (DragPhase::Drop, None) => {
                self.drag_target = None;
                self.drag_effect = DragDropEffects::NONE;
            }
            _ => {}
        }
        self.port.set_drag_effect(effect);
    }

    /// The ErrorProvider adornment (DATA-2, WinForms' error icon adapted to Kubuno): a round
    /// danger-coloured glyph with an exclamation mark next to every control bound to a field in
    /// error ([`crate::scope::BindingProvider::field_error`]), blinking per its `BlinkStyle`, whose
    /// tooltip is the message.
    fn paint_error_glyphs(&mut self, canvas: &dyn ControlCanvas, frame: &Frame, scope: &ComponentScope, services: &mut crate::common::FrameServices) {
        if services.bound.is_empty() || !scope.has_providers() {
            self.glyphs.clear();
            return;
        }
        let now = self.port.now_ms();
        let mut shown: Vec<(String, String, u64)> = Vec::new();
        let mut wake: Option<u32> = None;
        let mut tooltip: Option<(String, Rect, String)> = None;
        let theme = canvas.theme();
        let white = kubuno_controls::styled::D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        for (id, bounds, client, paths) in &services.bound {
            let Some(error) = paths.iter().find_map(|p| scope.field_error(p)) else { continue };
            let since = self.glyphs.iter().find(|(i, m, _)| i == id && *m == error.message).map_or(now, |(_, _, s)| *s);
            let (visible, next) = glyph_phase(error.blink, error.blink_rate, now.saturating_sub(since));
            if let Some(ms) = next {
                wake = Some(wake.map_or(ms, |w| w.min(ms)));
            }
            let r = glyph_rect(*bounds, error.alignment, error.padding);
            if visible {
                canvas.fill_rounded(&r, GLYPH / 2.0, &theme.danger);
                let mid = (r.left + r.right) / 2.0;
                canvas.fill_rect(&Rect::new(mid - 1.0, r.top + 3.5, mid + 1.0, r.top + 10.0), &white);
                canvas.fill_rect(&Rect::new(mid - 1.0, r.top + 11.5, mid + 1.0, r.top + 13.5), &white);
            }
            let (dx, dy) = (client.left - bounds.left, client.top - bounds.top);
            let hit = Rect::new(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy);
            if !frame.pointer_outside() && hit.contains(frame.mouse.0, frame.mouse.1) {
                tooltip = Some((error.message.clone(), hit, format!("{id}#error")));
            }
            shown.push((id.clone(), error.message, since));
        }
        if let Some((text, rect, element)) = tooltip {
            services.tooltip = Some((u32::MAX, text, rect, element));
        }
        if let Some(ms) = wake {
            self.port.request_repaint_after(ms.max(1));
        }
        self.glyphs = shown;
    }

    /// After the paint: Enter/Escape nobody used click the view's AcceptButton/CancelButton; then
    /// the pointer shape, the tooltip, the accessibility tree, the window's `Form` properties, file
    /// drops, and the open context menu (painted in its floating surface).
    fn window_output(&mut self, canvas: Option<&dyn ControlCanvas>, frame: &Frame, view: &CompiledView, services: &crate::common::FrameServices, d: &mut Dispatch<'_>) {
        use kubuno_controls::host::vk;
        let now = self.port.now_ms();

        // Default buttons: the keys the controls left in the queue.
        for e in self.port.events() {
            let InputEvent::Key { vk: k, down: true, mods, .. } = &e else { continue };
            if !mods.is_none() {
                continue;
            }
            let button = if *k == vk::ENTER && !self.router.focused_clicks_on_enter(self.focus.focused()) {
                view.form.accept_button.as_deref()
            } else if *k == vk::ESCAPE {
                view.form.cancel_button.as_deref()
            } else {
                None
            };
            let Some((id, ..)) = button.and_then(|name| self.router.find_named(name)) else { continue };
            if self.router.raise_on(d, &id, "OnClick", crate::events::MouseEventArgs { button: crate::events::MouseButton::None, clicks: 0, x: 0.0, y: 0.0, delta: 0.0, mods: *mods }).is_some() {
                self.port.consume(&e);
                self.port.request_repaint_after(1);
            }
        }

        // Enter a text field used as a submit (a single-line field): the AcceptButton's too.
        if services.submitted {
            if let Some((id, ..)) = view.form.accept_button.as_deref().and_then(|name| self.router.find_named(name)) {
                let args = crate::events::MouseEventArgs { button: crate::events::MouseButton::None, clicks: 0, x: 0.0, y: 0.0, delta: 0.0, mods: frame.mods };
                if self.router.raise_on(d, &id, "OnClick", args).is_some() {
                    self.port.request_repaint_after(1);
                }
            }
        }

        // Shift+F10 or the context-menu key nobody used: the context menu of the focused element (or
        // of its nearest container that has one), opened from the keyboard below it.
        if self.menu.is_none() {
            for e in self.port.events() {
                let InputEvent::Key { vk: k, down: true, mods, .. } = &e else { continue };
                if !((*k == vk::APPS && mods.is_none()) || (*k == vk::F10 && *mods == host::Modifiers::SHIFT)) {
                    continue;
                }
                let focused = self.focus.focused().and_then(|f| services.access_ids.iter().find(|(_, _, id)| *id == Some(f)).map(|(_, element, _)| element.clone()));
                let target = match &focused {
                    Some(id) => services.context_menus.iter().rev().filter(|(owner, ..)| owner == id || id.starts_with(&format!("{owner}.")) || owner.is_empty()).max_by_key(|(owner, ..)| owner.len()).cloned(),
                    None => services.context_menus.iter().find(|(owner, ..)| owner.is_empty()).cloned(),
                };
                let Some((owner, rect, name)) = target else { continue };
                self.port.consume(&e);
                let mut request = crate::window::MenuRequest::new(name, crate::window::MenuAnchor::Point(rect.left + 8.0, (rect.top + 24.0).min(rect.bottom)));
                request.keyboard = true;
                request.owner = owner;
                crate::window::request_menu(request);
            }
        }

        if let Some((_, cursor)) = services.cursor {
            self.port.set_cursor(cursor);
        }

        // The `ToolTip` of the menu command the pointer or the keyboard rests on.
        if let (Some(menu), Some(c)) = (self.menu.as_ref(), canvas) {
            if let Some((path, since)) = &menu.rest {
                let tip = menu.spec.item_at(path).map(|i| i.tooltip.clone()).filter(|t| !t.is_empty());
                if let Some(text) = tip {
                    let waited = now.saturating_sub(*since);
                    if waited >= view.tooltips.initial_delay {
                        let row = menu.levels.last().and_then(|l| l.panel.zip(path.last()).and_then(|(p, &i)| l.menu.item_rect(p, i)));
                        let at = row.map_or(frame.mouse, |r| (r.right - 8.0, r.bottom - 6.0));
                        crate::window::paint_tooltip_at(c, frame, &text, at);
                    } else {
                        self.port.request_repaint_after(u32::try_from(view.tooltips.initial_delay - waited).unwrap_or(u32::MAX).max(1));
                    }
                }
            }
        }

        // The tooltip of the deepest hovered element that has one.
        if frame.mouse_down || frame.right_down {
            self.tooltip.press(now);
        }
        let candidate = services.tooltip.as_ref().map(|(_, text, _, element)| (element.as_str(), text.as_str()));
        match self.tooltip.update(candidate, now, &view.tooltips, frame.window_focused) {
            crate::window::TooltipShow::Show { text, hide_in } => {
                if let (Some(c), None) = (canvas, &self.menu) {
                    crate::window::paint_tooltip(c, frame, &text);
                }
                if let Some(ms) = hide_in {
                    self.port.request_repaint_after(u32::try_from(ms).unwrap_or(u32::MAX).max(1));
                }
            }
            crate::window::TooltipShow::Wait(Some(ms)) => self.port.request_repaint_after(u32::try_from(ms).unwrap_or(u32::MAX).max(1)),
            crate::window::TooltipShow::Wait(None) => {}
        }

        // The accessibility tree (built only when assistive technology asks, by the host).
        let mut focused = self.focus.focused().and_then(|f| services.access_ids.iter().find(|(_, _, id)| *id == Some(f)).map(|(a, ..)| *a));
        let mut nodes = services.access.clone();
        // The open menu: a Menu per level, a MenuItem per row (checked, expanded, its mnemonic); the
        // hot row holds the focus, so a screen reader reads the menu as the arrows move.
        self.menu_access.clear();
        if let Some(menu) = self.menu.as_ref() {
            let (menu_nodes, rows, hot) = crate::window::menu_access_nodes(menu);
            nodes.extend(menu_nodes);
            self.menu_access = rows;
            focused = hot.or(focused);
        }
        let form = view.form.options_in(&*d.vm, canvas.map(|c| c.theme()));
        self.port.publish_access(host::access::AccessTree {
            nodes,
            focus: focused,
            title: form.title.clone().unwrap_or_default(),
            scale: frame.scale,
        });
        self.port.set_form(form);
        self.port.accept_files(!services.drop_targets.is_empty());

        if let (Some(menu), Some(c)) = (self.menu.as_mut(), canvas) {
            let spec = menu.spec.clone();
            let mut handler = MenuOwnerDraw { d, spec: &spec };
            menu.paint(c, frame, Some(&mut handler));
        }
    }
}

/// The owner-draw handler of an open context menu (`OwnerDraw`, EVT-8): each item to its
/// `OnDrawItem` handler, the `<ContextMenu>` as the sender.
struct MenuOwnerDraw<'x, 'd> {
    d: &'x mut Dispatch<'d>,
    spec: &'x crate::window::MenuSpec,
}

impl kubuno_ui::graphics::OwnerDrawHandler for MenuOwnerDraw<'_, '_> {
    fn draw_item(&mut self, e: &mut kubuno_ui::graphics::DrawItemEventArgs<'_>) {
        let Some(handler) = self.spec.draw_item.as_deref() else {
            e.draw_default = true;
            return;
        };
        let d = &mut *self.d;
        let sender = ElementRef { name: Some(&self.spec.name), element: "ContextMenu", id: &self.spec.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
        crate::events::DrawItemEventArgs::lend(e, |args| {
            d.handlers.dispatch_args(handler, &mut *d.vm, &sender, args);
        });
    }
}

/// Runs a context menu command: its `OnClick` handler, with the `<MenuItem>` as the sender, and
/// reports it in the frame's events.
/// Row `i`'s sub-menu of the open menu just opened: its `OnDropDownOpening` handler runs, then the
/// sub-menu is read again from the view (its bindings, its `ItemsSource`), so the handler can fill it.
/// The menu named `name`: the page's, else one of the user controls painted last frame (by its key, which the
/// elements of its own view name, else by its own name, which its code opens it by), with where its handlers run.
fn find_menu(view: &CompiledView, last: &crate::common::FrameServices, name: &str) -> Option<(crate::window::MenuSpec, Option<std::rc::Rc<dyn crate::events::router::DispatchScope>>)> {
    if let Some(spec) = view.menus.iter().find(|m| m.name == name) {
        return Some((spec.clone(), None));
    }
    let local = last.local_menus.iter().find(|m| m.key == name).or_else(|| last.local_menus.iter().find(|m| m.name == name))?;
    Some(((*local.spec).clone(), local.scope.clone()))
}

/// Runs `f` with the dispatch of `scope` (a user control's own menu), else with `d` (the page's).
fn scoped(d: &mut Dispatch<'_>, scope: Option<&std::rc::Rc<dyn crate::events::router::DispatchScope>>, f: &mut dyn FnMut(&mut Dispatch<'_>)) {
    match scope {
        Some(scope) => scope.with_dispatch(d, f),
        None => f(d),
    }
}

fn open_submenu(d: &mut Dispatch<'_>, menu: &mut crate::window::OpenMenu, path: &[usize], checks: &std::collections::HashMap<String, bool>) {
    let Some(item) = menu.spec.item_at(path) else { return };
    let Some(source) = menu.source.find(&item.id).cloned() else { return };
    if let Some(handler) = &source.on_drop_down_opening {
        let sender = ElementRef { name: source.name.as_deref(), element: "MenuItem", id: &source.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
        d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut crate::events::CancelEventArgs::default());
    }
    let fresh = crate::window::MenuSpec { items: vec![source], ..menu.source.clone() }.resolved(&*d.vm, checks);
    if let Some(item) = fresh.items.into_iter().next() {
        if let Some(slot) = menu.spec.item_at_mut(path) {
            *slot = item;
        }
        menu.rebuild();
    }
}

/// A menu item was chosen: its check mark (`CheckOnClick`, `RadioGroup` among `siblings`), its
/// `OnClick`, and for an item of an `ItemsSource`, the menu's `OnItemClicked` with its key.
fn choose_menu_item(
    d: &mut Dispatch<'_>,
    menu: &crate::window::MenuSpec,
    item: &crate::window::MenuItemSpec,
    siblings: &[crate::window::MenuItemSpec],
    checks: &mut std::collections::HashMap<String, bool>,
    frame: &Frame,
    commands: &[crate::menus::CommandSpec],
) {
    let mut changed: Vec<(&crate::window::MenuItemSpec, bool)> = Vec::new();
    if !item.radio_group.is_empty() {
        for s in siblings.iter().filter(|s| s.radio_group == item.radio_group) {
            let on = s.id == item.id;
            if s.checked != on {
                changed.push((s, on));
            }
        }
    } else if item.check_on_click {
        changed.push((item, !item.checked));
    }
    for (s, on) in changed {
        checks.insert(s.id.clone(), on);
        if let Some((_, spec)) = s.bindings.iter().find(|(p, _)| *p == "Checked").filter(|(_, spec)| spec.mode.writes_back()) {
            spec.update_source(d.vm, crate::binding::Value::Bool(on));
        }
        if let Some(handler) = &s.on_checked_changed {
            let sender = ElementRef { name: s.name.as_deref(), element: "MenuItem", id: &s.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
            d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut crate::events::CheckedChangedEventArgs::new(!on, on, crate::events::ChangeSource::User));
        }
    }
    if let (Some(key), Some(handler)) = (&item.row_key, &menu.on_item_clicked) {
        let sender = ElementRef { name: Some(&menu.name), element: "ContextMenu", id: &menu.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
        let mut args = crate::events::TextChangedEventArgs::new(String::new(), key.clone(), crate::events::ChangeSource::User);
        d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut args);
    }
    run_menu_item(d, item, frame);
    // Its command (after its own Click, as a ribbon control does): `Execute`, a checkable one switches.
    if let Some(cmd) = item.command.as_deref().and_then(|n| commands.iter().find(|c| c.name == n)) {
        execute_command(d, cmd, checks);
    }
}

fn run_menu_item(d: &mut Dispatch<'_>, item: &crate::window::MenuItemSpec, frame: &Frame) {
    let Some(handler) = &item.handler else { return };
    let sender = ElementRef { name: item.name.as_deref(), element: "MenuItem", id: &item.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
    let mut args = crate::events::MouseEventArgs { button: crate::events::MouseButton::Left, clicks: 1, x: 0.0, y: 0.0, delta: 0.0, mods: frame.mods };
    d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut args);
    d.events.push(ViewEvent {
        focus_id: None,
        handler: Some(handler.clone()),
        kind: crate::node::ViewEventKind::Other { name: "Click", args: Rc::new(args) },
    });
}

/// Runs a `<Command>` of the view (a menu item naming it was chosen, or its shortcut pressed): a
/// checkable one switches its `Checked` (written back to a two-way binding, else kept here), then its
/// `OnExecute` runs with the command as the sender. Nothing when it is disabled.
fn execute_command(d: &mut Dispatch<'_>, cmd: &crate::menus::CommandSpec, checks: &mut std::collections::HashMap<String, bool>) {
    let now = cmd.resolved(&*d.vm, checks);
    if !now.enabled {
        return;
    }
    if cmd.checkable {
        match cmd.checked_binding() {
            Some(spec) if spec.mode.writes_back() => spec.update_source(d.vm, crate::binding::Value::Bool(!now.checked)),
            _ => {
                checks.insert(cmd.id.clone(), !now.checked);
            }
        }
    }
    let Some(handler) = &cmd.on_execute else { return };
    let sender = ElementRef { name: Some(&cmd.name), element: "Command", id: &cmd.id, bounds: Rect::default(), focus_id: None, attributes: &[] };
    d.handlers.dispatch_args(handler, &mut *d.vm, &sender, &mut crate::events::EmptyEventArgs);
    d.events.push(ViewEvent { focus_id: None, handler: Some(handler.clone()), kind: crate::node::ViewEventKind::Clicked });
}

/// Who raises a menu's `OnClosed` (its handler, sender and identity), to tell when it closed.
#[derive(Debug, Clone, PartialEq)]
struct ClosedHandler {
    menu: String,
    handler: Option<String>,
    element: &'static str,
    name: Option<String>,
    id: String,
}

fn closed_handler(menu: &crate::window::OpenMenu) -> ClosedHandler {
    let (element, name) = menu.spec.sender_element();
    ClosedHandler { menu: menu.spec.name.clone(), handler: menu.spec.on_closed.clone(), element, name: name.map(str::to_string), id: menu.spec.id.clone() }
}

/// The next enabled item of a menu bar after `index` (before it when `forward` is false), wrapping.
fn next_enabled(items: &[crate::menus::BarItem], index: usize, forward: bool) -> Option<usize> {
    let n = items.len();
    if n == 0 {
        return None;
    }
    (1..=n).map(|step| if forward { (index.wrapping_add(step)) % n } else { (index + n * 2 - step % n) % n }).find(|&j| items.get(j).is_some_and(|i| i.enabled))
}

/// The path (row of each level) of the item of id `id` among `items`.
fn path_of(items: &[crate::window::MenuItemSpec], id: &str) -> Option<Vec<usize>> {
    for (i, item) in items.iter().enumerate() {
        if item.id == id {
            return Some(vec![i]);
        }
        if let Some(mut rest) = path_of(&item.children, id) {
            rest.insert(0, i);
            return Some(rest);
        }
    }
    None
}

/// Polls one `.kbview` file's mtime and reloads a [`Runtime`] when it moves —
/// the file-system half of hot reload, kept separate from [`Runtime`] itself
/// so the interpreter's own tests need no filesystem (see `runtime`'s own
/// tests, all on [`Runtime::reload_from_text`] directly).
pub struct FileWatcher {
    path: PathBuf,
    last_mtime: Option<SystemTime>,
}

impl FileWatcher {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), last_mtime: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn mtime(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    /// Reads the file and reloads `runtime` if its mtime has moved since the
    /// last check (or this is the first check at all). Returns `true` when a
    /// reload was *attempted* (whether or not it succeeded — check
    /// `runtime.diagnostics()` for that); `false` when nothing changed, or
    /// the file could not be read (its own error, if any, is folded into
    /// `runtime`'s diagnostics rather than panicking the caller).
    pub fn poll(&mut self, runtime: &mut Runtime) -> bool {
        let current = self.mtime();
        if current.is_some() && current == self.last_mtime {
            return false;
        }
        self.last_mtime = current;
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                runtime.set_base_dir(self.path.parent().map(Path::to_path_buf));
                runtime.reload_from_text(&text);
                true
            }
            Err(e) => {
                runtime.diagnostics = vec![Diagnostic {
                    range: rowan::TextRange::new(0.into(), 0.into()),
                    line: 1,
                    column: 1,
                    message: format!("could not read {}: {e}", self.path.display()),
                }];
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_no_view() {
        let rt = Runtime::new();
        assert!(!rt.has_view());
        assert!(rt.diagnostics().is_empty());
    }

    #[test]
    fn successful_reload_has_a_view_and_no_diagnostics() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(r#"<Button Text="Ok"/>"#));
        assert!(rt.has_view());
        assert!(rt.diagnostics().is_empty());
    }

    #[test]
    fn a_bad_reload_keeps_the_last_good_tree() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(r#"<Card Title="A"><Stack/></Card>"#));
        assert!(rt.has_view());

        assert!(!rt.reload_from_text(r#"<Card Title="B"><Frobnicator/></Card>"#));
        // The tree from the successful reload is still there...
        assert!(rt.has_view());
        // ...and the diagnostics are the NEW file's, surfaced for the banner.
        assert!(!rt.diagnostics().is_empty());
        assert!(rt.diagnostics()[0].message.contains("Frobnicator"), "{:?}", rt.diagnostics());
    }

    #[test]
    fn a_later_good_reload_clears_the_diagnostics() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(r#"<Button Text="Ok"/>"#));
        assert!(!rt.reload_from_text(r#"<Frobnicator/>"#));
        assert!(!rt.diagnostics().is_empty());
        assert!(rt.reload_from_text(r#"<Button Text="Retry"/>"#));
        assert!(rt.diagnostics().is_empty());
    }

    #[test]
    fn design_size_is_the_declared_one() {
        let mut rt = Runtime::new();
        assert_eq!(rt.design_size(), None);
        assert!(rt.reload_from_text(r#"<Button Text="Ok"/>"#));
        assert_eq!(rt.design_size(), None);
        assert!(rt.reload_from_text(r#"<Card DesignWidth="800" DesignHeight="450"><Stack/></Card>"#));
        assert_eq!(rt.design_size(), Some((800.0, 450.0)));
    }

    #[test]
    fn broken_from_the_start_never_has_a_view() {
        let mut rt = Runtime::new();
        assert!(!rt.reload_from_text(r#"<Frobnicator/>"#));
        assert!(!rt.has_view());
        assert!(!rt.diagnostics().is_empty());
    }

    // ── EVT-6: lifecycle, dispatcher, timers, async handlers, on a fake host ────────────

    use crate::events::{delay, spawn_local, DispatchError, FormClosedEventArgs, FormClosingEventArgs, UiHandle};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// What the fake host recorded, and the input it hands to the next frame.
    #[derive(Default)]
    struct FakeState {
        now: Cell<u64>,
        pending: RefCell<Vec<InputEvent>>,
        frame_events: RefCell<Vec<(InputEvent, bool)>>,
        wake_after: RefCell<Vec<u32>>,
        repaint_after: RefCell<Vec<u32>>,
        deferred: Cell<bool>,
        cancelled: Cell<u32>,
        quit: Cell<bool>,
        cursor: Cell<Option<host::Cursor>>,
        form: RefCell<Option<host::FormOptions>>,
        access: RefCell<Option<host::access::AccessTree>>,
        access_actions: RefCell<Vec<(u64, host::access::AccessAction)>>,
        accept_files: Cell<bool>,
        drag: RefCell<Option<host::dnd::DragFrame>>,
        drag_effect: Cell<crate::events::DragDropEffects>,
        /// The window is hidden (started hidden, `Hide()`): its frames run off screen.
        hidden: Cell<bool>,
    }

    struct FakeHost(Rc<FakeState>);

    impl HostPort for FakeHost {
        fn events(&self) -> Vec<InputEvent> {
            self.0.frame_events.borrow().iter().filter(|(_, used)| !used).map(|(e, _)| e.clone()).collect()
        }
        fn consume(&self, target: &InputEvent) {
            if let Some(slot) = self.0.frame_events.borrow_mut().iter_mut().find(|(e, used)| !used && e == target) {
                slot.1 = true;
            }
        }
        fn now_ms(&self) -> u64 {
            self.0.now.get()
        }
        fn request_repaint_after(&self, ms: u32) {
            self.0.repaint_after.borrow_mut().push(ms);
        }
        fn request_wake_after(&self, ms: u32) {
            self.0.wake_after.borrow_mut().push(ms);
        }
        fn defer_close(&self) {
            self.0.deferred.set(true);
        }
        fn cancel_close(&self) {
            self.0.cancelled.set(self.0.cancelled.get() + 1);
        }
        fn quit(&self) {
            self.0.quit.set(true);
        }
        fn set_cursor(&self, cursor: host::Cursor) {
            self.0.cursor.set(Some(cursor));
        }
        fn set_form(&self, form: host::FormOptions) {
            *self.0.form.borrow_mut() = Some(form);
        }
        fn publish_access(&self, tree: host::access::AccessTree) {
            *self.0.access.borrow_mut() = Some(tree);
        }
        fn take_access_actions(&self) -> Vec<(u64, host::access::AccessAction)> {
            std::mem::take(&mut *self.0.access_actions.borrow_mut())
        }
        fn accept_files(&self, accept: bool) {
            self.0.accept_files.set(accept);
        }
        fn drag(&self) -> Option<host::dnd::DragFrame> {
            self.0.drag.borrow().clone()
        }
        fn set_drag_effect(&self, effect: crate::events::DragDropEffects) {
            self.0.drag_effect.set(effect);
        }
        fn window_visible(&self) -> bool {
            !self.0.hidden.get()
        }
    }

    #[derive(Default)]
    struct Vm {
        log: Vec<String>,
        dirty: bool,
    }

    impl ViewModel for Vm {
        fn get(&self, _: &str) -> Option<crate::binding::Value> {
            None
        }
        fn set(&mut self, _: &str, _: crate::binding::Value) {}
    }

    #[crate::event_handlers]
    impl Vm {
        fn loaded(&mut self) {
            self.log.push("Load".into());
        }
        fn activated(&mut self) {
            self.log.push("Activated".into());
        }
        fn shown(&mut self) {
            self.log.push("Shown".into());
        }
        fn deactivated(&mut self) {
            self.log.push("Deactivate".into());
        }
        fn closing(&mut self, e: &mut FormClosingEventArgs) {
            self.log.push(format!("FormClosing {:?}", e.reason));
            e.cancel = self.dirty;
        }
        fn closed(&mut self, e: &FormClosedEventArgs) {
            self.log.push(format!("FormClosed {:?}", e.reason));
        }
        fn tick(&mut self) {
            self.log.push("Tick".into());
        }
        fn source_changing(&mut self, e: &mut crate::events::CancelEventArgs) {
            self.log.push("Changing".into());
            e.cancel = self.dirty;
        }
        async fn load_async(ui: UiHandle<Self>) {
            ui.update(|vm| vm.log.push("async start".into()));
            delay(Duration::from_millis(1000)).await;
            ui.update(|vm| vm.log.push("async end".into()));
        }
        async fn tick_async(ui: UiHandle<Self>, e: crate::events::EmptyEventArgs) {
            let _copy: crate::events::EmptyEventArgs = e;
            ui.update(|vm| vm.log.push("async tick".into()));
        }
        // Drag and drop, owner-draw, paint (EVT-8).
        fn drag_enter_a(&mut self, e: &mut crate::events::DragEventArgs) {
            self.log.push(format!("A.DragEnter {}", e.data.text.as_deref().unwrap_or_default()));
            e.effect = crate::events::DragDropEffects::COPY;
        }
        fn drag_over_a(&mut self, e: &mut crate::events::DragEventArgs) {
            self.log.push(format!("A.DragOver {:?}", e.effect));
        }
        fn drag_leave_a(&mut self) {
            self.log.push("A.DragLeave".into());
        }
        fn drag_enter_b(&mut self, e: &mut crate::events::DragEventArgs) {
            self.log.push("B.DragEnter".into());
            e.effect = crate::events::DragDropEffects::MOVE;
        }
        fn drag_drop_b(&mut self, e: &mut crate::events::DragEventArgs) {
            self.log.push(format!("B.DragDrop {:?} {}", e.effect, e.data.text.as_deref().unwrap_or_default()));
        }
        fn draw_row(&mut self, e: &mut crate::events::DrawItemEventArgs) {
            self.log.push(format!("DrawItem {:?} {}", e.index, e.text));
            e.graphics().fill_rectangle(kubuno_ui::graphics::Color::RED, e.bounds);
        }
        fn measure_row(&mut self, e: &mut crate::events::MeasureItemEventArgs) {
            e.item_height = 50.0;
            self.log.push(format!("MeasureItem {}", e.index));
        }
        // Menus (MENUS.md).
        fn menu_saved(&mut self) {
            self.log.push("Saved".into());
        }
        fn menu_quit(&mut self) {
            self.log.push("Quit".into());
        }
        fn cmd_find(&mut self) {
            self.log.push("Find".into());
        }
        fn bar_on(&mut self) {
            self.log.push("MenuActivate".into());
        }
        fn bar_off(&mut self) {
            self.log.push("MenuDeactivate".into());
        }
        fn menu_closed(&mut self) {
            self.log.push("Closed".into());
        }
        fn paint_box(&mut self, e: &mut crate::events::PaintEventArgs) {
            self.log.push(format!("Paint {}", e.is_painting()));
            e.graphics().draw_line(&kubuno_ui::graphics::Pen::new(kubuno_ui::graphics::Color::BLACK, 1.0), kubuno_ui::graphics::PointF::new(0.0, 0.0), kubuno_ui::graphics::PointF::new(5.0, 5.0));
        }
    }

    const VIEW: &str = r#"<Panel OnLoad="loaded" OnActivated="activated" OnShown="shown" OnDeactivate="deactivated" OnFormClosing="closing" OnFormClosed="closed"/>"#;

    struct Bench {
        rt: Runtime,
        host: Rc<FakeState>,
        wakes: Arc<AtomicUsize>,
        vm: Vm,
    }

    impl Bench {
        fn new(xml: &str) -> Self {
            let host = Rc::new(FakeState::default());
            let wakes = Arc::new(AtomicUsize::new(0));
            let w = wakes.clone();
            let mut rt = Runtime::with_port(Box::new(FakeHost(host.clone())), Box::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            }));
            assert!(rt.reload_from_text(xml), "{:?}", rt.diagnostics());
            Self { rt, host, wakes, vm: Vm::default() }
        }

        /// One frame at `now` (the fake clock), with the host events queued since the last one.
        fn frame(&mut self, now: u64) -> Vec<ViewEvent> {
            self.host.now.set(now);
            self.host.deferred.set(false);
            let pending: Vec<InputEvent> = self.host.pending.borrow_mut().drain(..).collect();
            *self.host.frame_events.borrow_mut() = pending.into_iter().map(|e| (e, false)).collect();
            let frame = Frame {
                size: (400.0, 300.0),
                mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
                mouse_down: false,
                right_down: false,
                middle_down: false,
                dismiss: false,
                scale: 1.0,
                client_origin: (0.0, 0.0),
                work_area: (0.0, 0.0, 400.0, 300.0),
                chrome_top: 0.0,
                mods: host::Modifiers::NONE,
                wheel: (0.0, 0.0),
                click_count: 0,
                window_focused: !self.host.hidden.get(),
            };
            let mut handlers = HandlerTable::new();
            self.rt.run_frame(None, &frame, &mut TypedVm(&mut self.vm), &mut handlers, Rect::new(0.0, 0.0, 400.0, 300.0), None)
        }

        /// One frame that also paints (on a recording canvas), with the pointer at `mouse`.
        fn paint_frame(&mut self, now: u64, canvas: &kubuno_ui::graphics::testing::RecordingCanvas) -> Vec<ViewEvent> {
            self.host.now.set(now);
            let pending: Vec<InputEvent> = self.host.pending.borrow_mut().drain(..).collect();
            *self.host.frame_events.borrow_mut() = pending.into_iter().map(|e| (e, false)).collect();
            let frame = Frame {
                size: (400.0, 600.0),
                mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
                mouse_down: false,
                right_down: false,
                middle_down: false,
                dismiss: false,
                scale: 1.0,
                client_origin: (0.0, 0.0),
                work_area: (0.0, 0.0, 400.0, 600.0),
                chrome_top: 0.0,
                mods: host::Modifiers::NONE,
                wheel: (0.0, 0.0),
                click_count: 0,
                window_focused: !self.host.hidden.get(),
            };
            let mut handlers = HandlerTable::new();
            self.rt.run_frame(Some(canvas), &frame, &mut TypedVm(&mut self.vm), &mut handlers, Rect::new(0.0, 0.0, 400.0, 600.0), None)
        }

        fn take_log(&mut self) -> Vec<String> {
            std::mem::take(&mut self.vm.log)
        }

        fn request_close(&self, reason: host::CloseReason) {
            self.host.pending.borrow_mut().push(InputEvent::CloseRequested(reason));
        }
    }

    /// A window started hidden (a tray application at logon): its first frame runs off screen and
    /// raises `Load` there, before anything is painted; `Shown` waits for the first frame on screen,
    /// and neither comes back when the window is hidden and shown again.
    #[test]
    fn a_hidden_window_gets_load_before_any_paint_and_shown_only_when_shown() {
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        let mut b = Bench::new(r#"<Stack OnLoad="loaded" OnShown="shown" OnActivated="activated"><PaintBox Height="40" OnPaint="paint_box"/></Stack>"#);
        b.host.hidden.set(true);
        b.paint_frame(0, &canvas);
        assert_eq!(b.take_log(), ["Load", "Paint true"], "Load first, before the first paint; no Shown while hidden");
        b.paint_frame(16, &canvas);
        b.frame(32);
        assert_eq!(b.take_log(), ["Paint true"], "Load once, and still no Shown");

        b.host.hidden.set(false);
        b.paint_frame(48, &canvas);
        assert_eq!(b.take_log(), ["Activated", "Paint true", "Shown"], "Shown at the first frame on screen");

        // Hide, then show again: neither Load nor Shown is raised a second time.
        b.host.hidden.set(true);
        b.frame(64);
        b.host.hidden.set(false);
        b.frame(80);
        let log = b.take_log();
        assert!(!log.iter().any(|l| l == "Load" || l == "Shown"), "{log:?}");
    }

    #[test]
    fn the_view_lifecycle_runs_in_winforms_order_and_closing_ends_it() {
        let mut b = Bench::new(VIEW);
        b.frame(0);
        assert_eq!(b.take_log(), ["Load", "Activated", "Shown"]);
        assert!(b.host.deferred.get(), "a routed frame declares that closing goes through FormClosing");
        b.frame(16);
        assert!(b.take_log().is_empty());

        b.request_close(host::CloseReason::UserClosing);
        b.frame(32);
        assert_eq!(b.take_log(), ["FormClosing UserClosing", "FormClosed UserClosing", "Deactivate"]);
        assert!(b.host.quit.get(), "the host loop is asked to end");
        assert!(b.host.frame_events.borrow().iter().all(|(_, used)| *used), "the close request is consumed");
        assert!(!b.host.deferred.get(), "a closed view no longer defers closes");
        assert!(b.rt.is_closed());
        b.frame(48);
        assert!(b.take_log().is_empty(), "a closed view raises nothing");
    }

    #[test]
    fn a_cancelled_form_closing_keeps_the_view_open() {
        let mut b = Bench::new(VIEW);
        b.frame(0);
        b.take_log();
        b.vm.dirty = true;
        b.request_close(host::CloseReason::UserClosing);
        b.frame(10);
        assert_eq!(b.take_log(), ["FormClosing UserClosing"]);
        assert!(!b.host.quit.get() && !b.rt.is_closed());
        assert_eq!(b.host.cancelled.get(), 1, "the host is told (it refuses a session end with it)");
        assert!(b.host.deferred.get());

        // Saved: a close from code goes through, with its reason.
        b.vm.dirty = false;
        b.rt.close(CloseReason::ApplicationExitCall);
        assert!(b.wakes.load(Ordering::SeqCst) > 0, "Runtime::close wakes the host");
        b.frame(20);
        assert_eq!(b.take_log(), ["FormClosing ApplicationExitCall", "FormClosed ApplicationExitCall", "Deactivate"]);
        assert!(b.host.quit.get());
    }

    #[test]
    fn rust_subscribers_see_form_closing_and_may_cancel_it() {
        let mut b = Bench::new(VIEW);
        b.frame(0);
        b.take_log();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        b.rt.form_closing()
            .subscribe(move |sender, e| {
                s.borrow_mut().push(format!("{} {:?}", sender.element, e.reason));
                e.cancel = true;
            })
            .detach();
        b.request_close(host::CloseReason::WindowsShutDown);
        b.frame(10);
        assert_eq!(*seen.borrow(), ["Panel WindowsShutDown"]);
        assert!(!b.rt.is_closed());
    }

    #[test]
    fn closures_posted_from_threads_run_in_order_at_the_next_frame() {
        let mut b = Bench::new("<Panel/>");
        b.frame(0);
        let dispatcher = b.rt.dispatcher::<Vm>();
        let workers: Vec<_> = (0..3)
            .map(|t| {
                let d = dispatcher.clone();
                std::thread::spawn(move || {
                    assert!(d.invoke_required());
                    for i in 0..5 {
                        drop(d.begin_invoke(move |vm: &mut Vm| vm.log.push(format!("{t}.{i}"))));
                    }
                })
            })
            .collect();
        for w in workers {
            assert!(w.join().is_ok());
        }
        assert!(b.wakes.load(Ordering::SeqCst) >= 15, "each post wakes the host");
        b.frame(16);
        let log = b.take_log();
        assert_eq!(log.len(), 15);
        for t in 0..3 {
            let mine: Vec<&String> = log.iter().filter(|l| l.starts_with(&format!("{t}."))).collect();
            let expected: Vec<String> = (0..5).map(|i| format!("{t}.{i}")).collect();
            assert_eq!(mine, expected.iter().collect::<Vec<_>>(), "per-thread posting order is kept");
        }

        // invoke: the worker blocks until a frame ran its closure, and gets the typed result.
        let d = dispatcher.clone();
        let worker = std::thread::spawn(move || d.invoke(|vm: &mut Vm| vm.log.len() + 100));
        let mut now = 16;
        while !worker.is_finished() && now < 16 + 5_000 {
            now += 1;
            b.frame(now);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(worker.join().ok(), Some(Ok(100)));
        assert_eq!(dispatcher.invoke(|_vm: &mut Vm| 0), Err(DispatchError::OnUiThread));
    }

    #[test]
    fn closing_releases_waiting_workers_and_refuses_new_closures() {
        let mut b = Bench::new(VIEW);
        b.frame(0);
        let dispatcher = b.rt.dispatcher::<Vm>();
        let queued = dispatcher.begin_invoke(|vm: &mut Vm| vm.log.push("too late".into()));
        b.rt.close(CloseReason::UserClosing);
        // The close comes first in the frame: the queued closure never runs.
        b.frame(10);
        assert!(!b.take_log().contains(&"too late".to_string()));
        assert_eq!(queued.wait(), Err(DispatchError::Closed));
        assert!(dispatcher.is_closed());
        let d = dispatcher.clone();
        assert_eq!(std::thread::spawn(move || d.invoke(|_vm: &mut Vm| 1)).join().ok(), Some(Err(DispatchError::Closed)));
    }

    #[test]
    fn dropping_the_runtime_is_a_graceful_shutdown() {
        let b = Bench::new("<Panel/>");
        let dispatcher = b.rt.dispatcher::<Vm>();
        let pending = dispatcher.begin_invoke(|_vm: &mut Vm| 1);
        drop(b);
        assert_eq!(pending.wait(), Err(DispatchError::Closed));
        assert!(dispatcher.is_closed());
    }

    #[test]
    fn an_untyped_frame_reports_closures_for_a_view_model_type() {
        let mut b = Bench::new("<Panel/>");
        let r = b.rt.dispatcher::<Vm>().begin_invoke(|_vm: &mut Vm| 1);
        let frame = Frame {
            size: (10.0, 10.0),
            mouse: (0.0, 0.0),
            mouse_down: false,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 10.0, 10.0),
            chrome_top: 0.0,
            mods: host::Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        };
        let mut map = crate::binding::MapViewModel::new();
        let mut handlers = HandlerTable::new();
        b.rt.run_frame(None, &frame, &mut DynVm(&mut map), &mut handlers, Rect::default(), None);
        assert_eq!(r.wait(), Err(DispatchError::WrongViewModel));
    }

    #[test]
    fn the_view_form_properties_reach_the_host_window_every_frame() {
        let mut b = Bench::new(r#"<Panel Title="Demo" StartPosition="CenterScreen" FormBorderStyle="FixedDialog" MaximizeBox="false" TopMost="true" Opacity="80" MinimumSize="320, 200"/>"#);
        b.frame(0);
        let form = b.host.form.borrow().clone().expect("the form is handed to the host");
        assert_eq!(form.title.as_deref(), Some("Demo"));
        assert_eq!(form.start_position, host::StartPosition::CenterScreen);
        assert_eq!(form.border_style, host::FormBorderStyle::FixedDialog);
        assert!(form.top_most && !form.maximize_box && form.minimize_box && form.control_box);
        assert!((form.opacity - 0.8).abs() < 1e-6);
        assert_eq!(form.min_client_size, Some((320.0, 200.0)));
        let tree = b.host.access.borrow().clone().expect("the accessibility tree is published");
        assert_eq!(tree.title, "Demo", "the window node is named after the title");
        assert!(!b.host.accept_files.get(), "nothing accepts dropped files");
    }

    #[test]
    fn a_view_without_form_properties_keeps_the_host_defaults() {
        let mut b = Bench::new("<Panel/>");
        b.frame(0);
        let form = b.host.form.borrow().clone().expect("the form is handed to the host");
        assert_eq!(form, host::FormOptions { title: None, ..host::FormOptions::default() });
    }

    #[test]
    fn a_timer_ticks_on_the_fake_clock_and_asks_to_be_woken() {
        let mut b = Bench::new("<Panel/>");
        let timer = Timer::new("clock").with_interval(1000).with_handler("tick");
        b.rt.add_timer(&timer);
        b.rt.add_timer(&timer);
        timer.start();
        b.frame(0);
        assert_eq!(*b.host.wake_after.borrow(), [1000], "woken for the first tick");
        b.frame(999);
        assert!(b.take_log().is_empty());
        let events = b.frame(1000);
        assert_eq!(b.take_log(), ["Tick"], "added twice, still one timer");
        assert!(events.iter().any(|e| e.handler.as_deref() == Some("tick")));
        assert_eq!(b.host.wake_after.borrow().last(), Some(&1000));
        b.frame(3500);
        assert_eq!(b.take_log(), ["Tick"], "late ticks coalesce");
        b.rt.remove_timer(&timer);
        b.frame(10_000);
        assert!(b.take_log().is_empty());
    }

    #[test]
    fn an_async_handler_awaits_a_delay_on_the_ui_thread_and_completes() {
        let mut b = Bench::new(r#"<Panel OnLoad="load_async"/>"#);
        b.frame(0);
        assert_eq!(b.take_log(), ["async start"], "started within the Load frame, up to its first await");
        assert_eq!(b.rt.ui.task_count(), 1);
        assert_eq!(b.host.wake_after.borrow().last(), Some(&1000), "the delay asks for a wake-up");
        b.frame(500);
        assert!(b.take_log().is_empty());
        b.frame(1000);
        assert_eq!(b.take_log(), ["async end"]);
        assert_eq!(b.rt.ui.task_count(), 0, "finished");
    }

    #[test]
    fn an_async_handler_gets_a_copy_of_the_args_from_a_timer_tick() {
        let mut b = Bench::new("<Panel/>");
        let timer = Timer::new("t").with_interval(10).with_handler("tick_async");
        b.rt.add_timer(&timer);
        timer.start();
        b.frame(0);
        b.frame(10);
        assert_eq!(b.take_log(), ["async tick"]);
    }

    #[test]
    fn closing_cancels_async_tasks_and_their_handles() {
        let mut b = Bench::new(&VIEW.replace(r#"OnLoad="loaded""#, r#"OnLoad="load_async""#));
        b.frame(0);
        assert!(b.take_log().contains(&"async start".to_string()));
        let ui = b.rt.ui_handle::<Vm>();
        let finished = Rc::new(Cell::new(false));
        let f = finished.clone();
        let handle = b.rt.spawn_local(async move {
            delay(Duration::from_millis(5000)).await;
            f.set(true);
            7
        });
        b.frame(10);
        assert!(!handle.is_finished() && !handle.is_cancelled());
        b.request_close(host::CloseReason::UserClosing);
        b.frame(20);
        assert_eq!(b.rt.ui.task_count(), 0, "every task was dropped at its await");
        assert!(handle.is_cancelled());
        assert!(ui.is_closed() && ui.update(|vm| vm.log.len()).is_none());
        b.frame(10_000);
        assert!(!finished.get());
        assert!(!b.take_log().contains(&"async end".to_string()));
    }

    #[test]
    fn spawn_local_inside_a_task_and_ui_handle_updates_are_scoped() {
        let mut b = Bench::new("<Panel/>");
        let ui = b.rt.ui_handle::<Vm>();
        assert!(ui.update(|vm| vm.log.len()).is_none(), "outside a pumped task there is no view model to lend");
        let outer = ui.clone();
        let handle = b.rt.spawn_local(async move {
            let inner = outer.clone();
            let child = spawn_local(async move {
                inner.update(|vm| vm.log.push("child".into()));
                2
            });
            let nested = outer.update(|_vm| outer.update(|_| ()).is_none());
            outer.update(|vm| vm.log.push(format!("nested update refused: {nested:?}")));
            child.await.unwrap_or(0) + 40
        });
        b.frame(0);
        b.frame(1);
        assert_eq!(b.take_log(), ["nested update refused: Some(true)", "child"]);
        assert_eq!(handle.try_take(), Some(42));
    }

    const DND_VIEW: &str = r#"<Stack>
  <Panel x:Name="a" Height="100" AllowDrop="true" OnDragEnter="drag_enter_a" OnDragOver="drag_over_a" OnDragLeave="drag_leave_a"/>
  <Panel x:Name="b" Height="100" AllowDrop="true" OnDragEnter="drag_enter_b" OnDragDrop="drag_drop_b"/>
  <Panel x:Name="c" Height="60"/>
</Stack>"#;

    fn drag_at(b: &Bench, phase: host::dnd::DragPhase, x: f32, y: f32) {
        use crate::events::{DataObject, DragDropEffects};
        *b.host.drag.borrow_mut() = Some(host::dnd::DragFrame {
            phase,
            data: Rc::new(DataObject::from_text("hi")),
            allowed: DragDropEffects::COPY | DragDropEffects::MOVE,
            x,
            y,
            mods: host::Modifiers::NONE,
            buttons: (true, false, false),
            internal: false,
        });
    }

    /// WinForms' drop-target sequence: DragEnter → DragOver* → DragLeave when the drag moves to
    /// another target (which gets its own DragEnter) → DragDrop with the effect it accepted; nothing
    /// for an element that does not allow drops.
    #[test]
    fn a_drag_is_routed_enter_over_leave_drop_to_the_elements_that_allow_drops() {
        use crate::events::DragDropEffects;
        use host::dnd::DragPhase;
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        let mut b = Bench::new(DND_VIEW);
        b.paint_frame(0, &canvas);
        b.take_log();
        let targets = b.rt.last.drop_targets.clone();
        assert_eq!(targets.len(), 2, "the two panels that allow drops: {:?}", targets.iter().map(|(id, _)| id).collect::<Vec<_>>());
        let (ra, rb) = (targets[0].1, targets[1].1);
        let (ax, ay) = ((ra.left + ra.right) / 2.0, (ra.top + ra.bottom) / 2.0);
        let (bx, by) = ((rb.left + rb.right) / 2.0, (rb.top + rb.bottom) / 2.0);

        drag_at(&b, DragPhase::Enter, ax, ay);
        b.paint_frame(16, &canvas);
        assert_eq!(b.take_log(), ["A.DragEnter hi"]);
        assert_eq!(b.host.drag_effect.get(), DragDropEffects::COPY, "the target's answer goes back to the source");

        drag_at(&b, DragPhase::Over, ax + 5.0, ay);
        b.paint_frame(32, &canvas);
        assert_eq!(b.take_log(), ["A.DragOver DragDropEffects(1)"], "DragOver starts from the last answer");

        drag_at(&b, DragPhase::Over, bx, by);
        b.paint_frame(48, &canvas);
        assert_eq!(b.take_log(), ["A.DragLeave", "B.DragEnter"]);
        assert_eq!(b.host.drag_effect.get(), DragDropEffects::MOVE);

        drag_at(&b, DragPhase::Drop, bx, by);
        b.paint_frame(64, &canvas);
        assert_eq!(b.take_log(), ["B.DragDrop DragDropEffects(2) hi"]);
        assert_eq!(b.host.drag_effect.get(), DragDropEffects::MOVE);

        // Over the panel that does not allow drops: refused, nothing raised.
        let rc = b.rt.last.drop_targets.iter().find(|(id, _)| id.ends_with('2')).map(|(_, r)| *r);
        assert!(rc.is_none());
        drag_at(&b, DragPhase::Enter, bx, 590.0);
        b.paint_frame(80, &canvas);
        assert!(b.take_log().is_empty());
        assert_eq!(b.host.drag_effect.get(), DragDropEffects::NONE);

        // A cancelled drag over A: enter, then leave.
        drag_at(&b, DragPhase::Enter, ax, ay);
        b.paint_frame(96, &canvas);
        drag_at(&b, DragPhase::Leave, ax, ay);
        b.paint_frame(112, &canvas);
        assert_eq!(b.take_log(), ["A.DragEnter hi", "A.DragLeave"]);
    }

    /// Owner-drawn items go to the element's DrawItem handler with the surface lent (MeasureItem first
    /// for OwnerDrawVariable), and a PaintBox raises Paint with its surface.
    #[test]
    fn owner_draw_and_paint_handlers_get_the_surface() {
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        let mut b = Bench::new(
            r#"<Stack>
  <ListBox Height="200" DrawMode="OwnerDrawVariable" OnDrawItem="draw_row" OnMeasureItem="measure_row"><Item Text="x"/><Item Text="y"/></ListBox>
  <PaintBox Height="40" OnPaint="paint_box"/>
</Stack>"#,
        );
        b.paint_frame(0, &canvas);
        let log = b.take_log();
        let measured: Vec<&String> = log.iter().filter(|l| l.starts_with("MeasureItem")).collect();
        assert_eq!(measured, ["MeasureItem 0", "MeasureItem 1"]);
        let drawn: Vec<&String> = log.iter().filter(|l| l.starts_with("DrawItem")).collect();
        assert_eq!(drawn, ["DrawItem Some(0) x", "DrawItem Some(1) y"]);
        let first_draw = log.iter().position(|l| l.starts_with("DrawItem")).unwrap_or(0);
        assert!(log.iter().position(|l| l.starts_with("MeasureItem")).unwrap_or(99) < first_draw, "measured before drawn");
        assert!(log.contains(&"Paint true".to_string()), "the PaintBox's handler drew with the lent surface: {log:?}");
        // The rows' fills went to the canvas (fallback surface: rectangles), 50 DIP high each.
        assert!(canvas.calls().iter().filter(|c| c.starts_with("fill_rounded(")).count() >= 2);
    }

    // ── DATA-2: the view's named components ─────────────────────────────────────────────────

    /// What `kubuno-data`'s `BindingSource` is to the runtime: a named non-visual component that
    /// answers `src.Value`, raises a cancelable `Changing`, and reports `"bad"` as a field error.
    #[derive(crate::component::Component, Default)]
    #[kubuno(extends = Component, overrides(Component))]
    struct RtSource {
        base: crate::component::ComponentCore,
        value: String,
    }

    impl crate::component::Component for RtSource {
        fn as_binding_provider(&self) -> Option<&dyn crate::scope::BindingProvider> {
            Some(self)
        }
        fn as_binding_provider_mut(&mut self) -> Option<&mut dyn crate::scope::BindingProvider> {
            Some(self)
        }
    }

    impl crate::scope::BindingProvider for RtSource {
        fn binding_get(&self, path: &str, want: crate::format::ValueKind, format: &crate::binding::BindingFormat, _: &crate::scope::ComponentScope) -> Option<crate::binding::Value> {
            (path == "Value").then(|| crate::format::to_target(crate::binding::Value::Str(self.value.clone()), want, format)).flatten()
        }
        fn binding_set(&mut self, path: &str, value: crate::binding::Value, _: &crate::binding::BindingFormat, _: &crate::scope::ComponentScope) -> bool {
            let crate::binding::Value::Str(text) = value else { return false };
            if path != "Value" {
                return false;
            }
            let mut args = crate::events::CancelEventArgs::default();
            if !crate::scope::raise_now("src", "OnChanging", &mut args) {
                self.base.queue_event("OnChanging", Box::new(args));
            }
            if !args.cancel {
                self.value = text;
            }
            true
        }
        fn field_error(&self, path: &str, _: &crate::scope::ComponentScope) -> Option<crate::scope::FieldError> {
            (path == "src.Value" && self.value == "bad").then(|| crate::scope::FieldError {
                message: "Not good.".into(),
                alignment: crate::scope::ErrorIconAlignment::MiddleRight,
                padding: 4.0,
                blink: crate::scope::ErrorBlinkStyle::NeverBlink,
                blink_rate: 250,
            })
        }
    }

    fn create_rt_source() -> Option<Rc<RefCell<dyn crate::component::Component>>> {
        Some(Rc::new(RefCell::new(RtSource::default())))
    }

    static RT_SOURCE_EVENTS: [crate::registry::EventMeta; 1] = [crate::registry::EventMeta::new("OnChanging", "Before a change.").args::<crate::events::CancelEventArgs>()];

    static RT_SOURCE: crate::registry::ClassRegistration = crate::registry::ClassRegistration {
        name: "RtSource",
        crate_name: "rt_test_app",
        kind: crate::registry::ClassKind::Component,
        doc: "A test data source.",
        extends: "Component",
        chain: <RtSource as crate::component::Lineage>::CHAIN,
        create: create_rt_source,
        properties: &[],
        events: &RT_SOURCE_EVENTS,
        default_event: None,
        default_property: None,
        toolbox_category: None,
        toolbox_icon: None,
        browsable: true,
        view: None,
        view_path: None,
        view_dir: None,
        source_file: "rt.rs",
    };

    const DATA_VIEW: &str = r#"<Panel DesignWidth="400" DesignHeight="300">
  <RtSource x:Name="src" OnChanging="source_changing"/>
  <TextField x:Name="field" Text="{Binding Source=src, Path=Value, Mode=TwoWay}" X="10" Y="10" Width="100" Height="30"/>
</Panel>"#;

    /// DATA-2: the runtime owns the named components of its view — bindings reach them by name,
    /// their XML handlers run synchronously on a binding write (and can cancel), events raised from
    /// code are delivered at the next frame, a field in error gets its glyph next to the bound
    /// control, and a hot reload keeps the instance.
    #[test]
    fn named_components_are_owned_bound_raise_their_handlers_and_show_their_errors() {
        crate::registry::register_class(&RT_SOURCE);
        let mut b = Bench::new(DATA_VIEW);
        let scope = b.rt.components();
        assert_eq!(scope.names(), ["src"]);
        // A control's two-way write goes through the same scoped view model the frame paints with.
        scope.view_model(&mut TypedViewModel(&mut b.vm), true).set("src.Value", crate::binding::Value::Str("one".into()));
        assert_eq!(b.take_log(), ["Changing"], "the XML handler ran synchronously");
        b.vm.dirty = true;
        scope.view_model(&mut TypedViewModel(&mut b.vm), true).set("src.Value", crate::binding::Value::Str("two".into()));
        assert_eq!(b.rt.with_component::<RtSource, _>("src", |s| s.value.clone()).as_deref(), Some("one"), "the handler cancelled the change");
        assert_eq!(scope.view_model(&mut TypedViewModel(&mut b.vm), true).get("src.Value"), Some(crate::binding::Value::Str("one".into())));
        b.vm.dirty = false;
        b.take_log();
        // From code (no view model at hand): queued, delivered by the next frame.
        b.rt.with_component::<RtSource, _>("src", |s| {
            use crate::scope::BindingProvider;
            s.binding_set("Value", crate::binding::Value::Str("three".into()), &Default::default(), &crate::scope::ComponentScope::new())
        });
        assert!(b.take_log().is_empty());
        b.frame(10);
        assert_eq!(b.take_log(), ["Changing"]);
        // A field in error: the glyph is painted at the right of the bound control.
        b.rt.with_component::<RtSource, _>("src", |s| s.value = "bad".into());
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(20, &canvas);
        let expected = glyph_rect(Rect::new(10.0, 10.0, 110.0, 40.0), ErrorIconAlignment::MiddleRight, 4.0);
        assert_eq!((expected.left, expected.top, expected.right, expected.bottom), (114.0, 17.0, 130.0, 33.0));
        assert!(canvas.calls().iter().any(|c| c == "fill_rounded(114,17,130,33 r=8)"), "{:?}", canvas.calls());
        assert_eq!(b.rt.glyphs.len(), 1);
        b.rt.with_component::<RtSource, _>("src", |s| s.value = "good".into());
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(30, &canvas);
        assert!(b.rt.glyphs.is_empty() && !canvas.calls().iter().any(|c| c == "fill_rounded(114,17,130,33 r=8)"));
        // A hot reload keeps the instance of an element with the same name and class.
        assert!(b.rt.reload_from_text(&DATA_VIEW.replace("Width=\"100\"", "Width=\"120\"")));
        assert_eq!(b.rt.with_component::<RtSource, _>("src", |s| s.value.clone()).as_deref(), Some("good"));
    }

    #[test]
    fn error_glyphs_are_placed_and_blink_like_winforms() {
        let r = Rect::new(10.0, 10.0, 110.0, 40.0);
        let at = |a| {
            let g = glyph_rect(r, a, 2.0);
            (g.left, g.top)
        };
        assert_eq!(at(ErrorIconAlignment::MiddleLeft), (-8.0, 17.0));
        assert_eq!(at(ErrorIconAlignment::TopRight), (112.0, 10.0));
        assert_eq!(at(ErrorIconAlignment::BottomLeft), (-8.0, 24.0));
        assert_eq!(glyph_phase(ErrorBlinkStyle::NeverBlink, 250, 100), (true, None));
        assert_eq!(glyph_phase(ErrorBlinkStyle::BlinkIfDifferentError, 250, 300), (false, Some(200)), "hidden in the second half-period");
        assert_eq!(glyph_phase(ErrorBlinkStyle::BlinkIfDifferentError, 250, 1600), (true, None), "three blinks, then steady");
        assert_eq!(glyph_phase(ErrorBlinkStyle::AlwaysBlink, 250, 1600).1, Some(150), "keeps blinking");
    }

    // ── Menus (vskubuno/docs/MENUS.md): shortcuts, the menu bar's keyboard, mnemonics ────────

    const MENU_VIEW: &str = r#"<Panel>
  <Command x:Name="cmd_find" Shortcut="Ctrl+F" OnExecute="cmd_find"/>
  <MenuBar x:Name="bar" OnMenuActivate="bar_on" OnMenuDeactivate="bar_off">
    <MenuItem Text="&amp;Fichier">
      <MenuItem Text="&amp;Enregistrer" ShortcutKeys="Ctrl+S" OnClick="menu_saved"/>
      <MenuItem Text="&amp;Quitter" OnClick="menu_quit"/>
    </MenuItem>
    <MenuItem Text="&amp;Aide"><MenuItem Text="À &amp;propos"/></MenuItem>
  </MenuBar>
  <ContextMenu x:Name="edit" OnClosed="menu_closed"><MenuItem Text="&amp;Couper"/></ContextMenu>
</Panel>"#;

    fn key(vk: u16, down: bool, mods: host::Modifiers) -> InputEvent {
        InputEvent::Key { vk, down, repeat: false, mods }
    }

    #[test]
    fn menu_shortcuts_run_their_item_and_commands_their_execute() {
        let mut b = Bench::new(MENU_VIEW);
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(0, &canvas);
        b.host.pending.borrow_mut().push(key(host::vk::letter('S'), true, host::Modifiers::CTRL));
        b.paint_frame(16, &canvas);
        assert_eq!(b.take_log(), ["Saved"]);
        assert!(b.host.frame_events.borrow().iter().all(|(_, used)| *used), "the shortcut is consumed");
        // A command no item runs, in a view without a ribbon.
        b.host.pending.borrow_mut().push(key(host::vk::letter('F'), true, host::Modifiers::CTRL));
        b.paint_frame(32, &canvas);
        assert_eq!(b.take_log(), ["Find"]);
        // Another chord is not a shortcut.
        b.host.pending.borrow_mut().push(key(host::vk::letter('S'), true, host::Modifiers::CTRL_SHIFT));
        b.paint_frame(48, &canvas);
        assert!(b.take_log().is_empty());
    }

    #[test]
    fn the_menu_bar_takes_the_keyboard_with_alt_and_its_mnemonics_open_and_choose() {
        let mut b = Bench::new(MENU_VIEW);
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(0, &canvas);
        // Alt pressed and released alone: the bar takes the keyboard on its first menu.
        b.host.pending.borrow_mut().extend([key(host::vk::MENU, true, host::Modifiers::ALT), key(host::vk::MENU, false, host::Modifiers::NONE)]);
        b.paint_frame(16, &canvas);
        assert_eq!(b.rt.bar_focus.as_ref().map(|(_, i)| *i), Some(0));
        assert_eq!(b.take_log(), ["MenuActivate"]);
        // Right moves to « Aide », Left back, Down opens « Fichier » with its first command hot.
        b.host.pending.borrow_mut().extend([key(host::vk::RIGHT, true, host::Modifiers::NONE), key(host::vk::LEFT, true, host::Modifiers::NONE), key(host::vk::DOWN, true, host::Modifiers::NONE)]);
        b.paint_frame(32, &canvas);
        let menu = b.rt.menu.as_ref().expect("the menu opened");
        assert_eq!(menu.bar.as_ref().map(|(_, i)| *i), Some(0));
        assert!(menu.keyboard);
        assert_eq!(menu.hot_path(), Some(vec![0]));
        // Right on a command without a sub-menu: the bar's next menu.
        b.host.pending.borrow_mut().push(key(host::vk::RIGHT, true, host::Modifiers::NONE));
        b.paint_frame(48, &canvas);
        assert_eq!(b.rt.menu.as_ref().and_then(|m| m.bar.as_ref().map(|(_, i)| *i)), Some(1));
        // Left back, then the mnemonic letter Q chooses « Quitter ».
        b.host.pending.borrow_mut().push(key(host::vk::LEFT, true, host::Modifiers::NONE));
        b.paint_frame(64, &canvas);
        b.host.pending.borrow_mut().push(key(host::vk::letter('Q'), true, host::Modifiers::NONE));
        b.paint_frame(80, &canvas);
        assert!(b.rt.menu.is_none());
        assert_eq!(b.take_log(), ["Quit", "MenuDeactivate"]);
        // Alt + F opens « Fichier » from the keyboard (its mnemonic); Escape closes it, the bar keeps the keyboard.
        b.host.pending.borrow_mut().push(key(host::vk::letter('F'), true, host::Modifiers::ALT));
        b.paint_frame(96, &canvas);
        b.paint_frame(112, &canvas);
        assert!(b.rt.menu.as_ref().is_some_and(|m| m.keyboard && m.bar.is_some()), "Alt+F opens the menu");
        b.host.pending.borrow_mut().push(key(host::vk::ESCAPE, true, host::Modifiers::NONE));
        b.paint_frame(128, &canvas);
        assert!(b.rt.menu.is_none() && b.rt.bar_focus.is_some());
        b.host.pending.borrow_mut().push(key(host::vk::ESCAPE, true, host::Modifiers::NONE));
        b.paint_frame(144, &canvas);
        assert!(b.rt.bar_focus.is_none());
    }

    #[test]
    fn a_context_menu_raises_closed_and_takes_the_keyboard_while_open() {
        let mut b = Bench::new(MENU_VIEW);
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(0, &canvas);
        crate::window::show_context_menu("edit", crate::window::MenuAnchor::Point(10.0, 10.0));
        b.paint_frame(16, &canvas);
        assert!(b.rt.menu.is_some());
        // While it is open, a typed letter is the menu's: nothing else sees it.
        b.host.pending.borrow_mut().extend([key(host::vk::letter('Z'), true, host::Modifiers::NONE), InputEvent::Text("z".into())]);
        b.paint_frame(32, &canvas);
        assert!(b.host.frame_events.borrow().iter().all(|(_, used)| *used));
        // The access tree holds the menu and its item.
        let tree = b.host.access.borrow().clone().expect("published");
        assert!(tree.nodes.iter().any(|n| n.role == host::access::AccessRole::Menu));
        assert!(tree.nodes.iter().any(|n| n.role == host::access::AccessRole::MenuItem && n.name == "Couper" && n.access_key.as_deref() == Some("C")));
        b.host.pending.borrow_mut().push(key(host::vk::ESCAPE, true, host::Modifiers::NONE));
        b.paint_frame(48, &canvas);
        assert!(b.rt.menu.is_none());
        assert_eq!(b.take_log(), ["Closed"]);
    }

    /// F6 and Alt alone (without a menu bar) move the focus to the title band's controls and back to the page
    /// (vskubuno docs/SHELL-CONTROLS.md §5).
    #[test]
    fn f6_and_alt_reach_the_title_band_and_give_the_focus_back() {
        use kubuno_controls::window_chrome::{ChromeStyle, SystemButtons};
        // A band to lay the regions out in (the test has no host window).
        crate::window::set_design_chrome(Some(crate::window::DesignChrome {
            style: ChromeStyle::default(),
            bounds: Rect::new(0.0, 0.0, 400.0, 600.0),
            has_icon: false,
            buttons: SystemButtons::default(),
        }));
        let mut b = Bench::new(
            r#"<Panel><TextField x:Name="page" X="10" Y="10" Width="200" Height="30"/><IconButton x:Name="bell" Icon="Bell" TitleBar.Region="Right" Width="36" Height="36"/></Panel>"#,
        );
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(0, &canvas);
        assert_eq!(b.rt.last.title_band_focus.len(), 1, "the band's button is known");
        let bell = b.rt.last.title_band_focus[0];
        // The page's field has the focus; F6 takes it to the band, F6 again gives it back.
        let field = b.rt.last.access_ids.iter().find(|(_, element, _)| element == "0").and_then(|(_, _, f)| *f).expect("the field is focusable");
        b.rt.focus.focus(field);
        b.paint_frame(16, &canvas);
        assert_eq!(b.rt.focus.focused(), Some(field));
        assert_ne!(field, bell);
        b.host.pending.borrow_mut().push(key(host::vk::F6, true, host::Modifiers::NONE));
        b.paint_frame(32, &canvas);
        assert_eq!(b.rt.focus.focused(), Some(bell));
        assert!(b.host.frame_events.borrow().iter().all(|(_, used)| *used), "F6 is consumed");
        b.host.pending.borrow_mut().push(key(host::vk::F6, true, host::Modifiers::NONE));
        b.paint_frame(48, &canvas);
        assert_eq!(b.rt.focus.focused(), Some(field));
        // Alt pressed and released alone: the band too (the view has no menu bar).
        b.host.pending.borrow_mut().push(key(host::vk::MENU, true, host::Modifiers::ALT));
        b.host.pending.borrow_mut().push(key(host::vk::MENU, false, host::Modifiers::NONE));
        b.paint_frame(64, &canvas);
        assert_eq!(b.rt.focus.focused(), Some(bell));
        crate::window::set_design_chrome(None);
    }

    /// The title bar's search button (`ShowSearch`) is a control of the window: in the accessibility tree, named by
    /// its tooltip, and reached by F6.
    #[test]
    fn the_title_bar_search_button_is_published_and_focusable() {
        use kubuno_controls::window_chrome::{ChromeStyle, SystemButtons};
        crate::window::set_design_chrome(Some(crate::window::DesignChrome {
            style: ChromeStyle::default(),
            bounds: Rect::new(0.0, 0.0, 400.0, 600.0),
            has_icon: false,
            buttons: SystemButtons::default(),
        }));
        let mut b = Bench::new(r#"<Panel ShowSearch="true" OnSearchClicked="find"><TextField X="10" Y="10" Width="200" Height="30"/></Panel>"#);
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        b.paint_frame(0, &canvas);
        let search = b.rt.last.access_ids.iter().find(|(_, element, _)| element.starts_with(crate::design::HEADER_ITEM_PREFIX)).cloned();
        assert!(search.as_ref().is_some_and(|(_, _, f)| f.is_some()), "{:?}", b.rt.last.access_ids);
        assert_eq!(b.rt.last.title_band_focus, vec![search.and_then(|s| s.2).expect("focusable")]);
        crate::window::set_design_chrome(None);
    }
}
