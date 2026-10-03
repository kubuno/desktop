//! [`Application`]: runs a form in a window (Windows Forms' `Application.Run`), and the per-window
//! driver shared by `run`, `show` and `show_dialog` — plus the windows a form draws INSIDE its own
//! (MDI documents, in-window dialogs), which reuse the same driver.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::SystemTime;

use kubuno_controls::host::{self, Chrome, Frame, HostOptions, StartPosition, POINTER_AWAY};
use kubuno_controls::window_chrome as wc;
use kubuno_controls::ControlCanvas;
use kubuno_ui::{Rect, Theme};
use kubuno_views::events::CloseReason;
use kubuno_views::runtime::Runtime;
use kubuno_views::window::WindowKind;

use crate::forms::{compose, DialogResult, Form};
use crate::View;

/// Application-wide settings, set before [`Application::run`] (Windows Forms'
/// `ApplicationConfiguration.Initialize()`).
#[derive(Clone)]
pub(crate) struct Settings {
    theme: Theme,
    chrome: Chrome,
    hot_reload: bool,
    diagnostics: bool,
}

thread_local! {
    static SETTINGS: RefCell<Settings> = RefCell::new(Settings {
        theme: Theme::light(),
        chrome: Chrome::Kubuno,
        hot_reload: cfg!(debug_assertions),
        diagnostics: true,
    });
    /// The forms open on this thread.
    static OPEN: RefCell<Vec<Form>> = const { RefCell::new(Vec::new()) };
}

fn settings() -> Settings {
    SETTINGS.with(|s| s.borrow().clone())
}

/// How [`Form::layout_mdi`] arranges the MDI documents (Windows Forms' `MdiLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdiLayout {
    /// Overlapping, each one a title band lower and to the right.
    Cascade,
    /// One above the other, full width.
    TileHorizontal,
    /// Side by side, full height.
    TileVertical,
    /// The minimised documents in a row at the bottom.
    ArrangeIcons,
}

/// A window message handler of a form (`Form::on_message`).
pub(crate) type MessageHandler = Box<dyn FnMut(&host::MessageArgs) -> Option<isize>>;

/// The per-frame painter of a view drawn inside another window.
type InnerPaint = Box<dyn FnMut(&dyn ControlCanvas, &Frame)>;

/// A window to open inside a form at its next frame (an MDI document, an in-window dialog).
pub(crate) struct InnerRequest {
    form: Form,
    /// An in-window dialog: it veils the page and takes all the input until it closes.
    modal: bool,
    /// Its page size (DIP).
    size: (f32, f32),
    make: Box<dyn FnOnce(&Settings) -> InnerPaint>,
    on_closed: Option<Box<dyn FnOnce(DialogResult)>>,
}

/// Runs a Kubuno application — Windows Forms' `Application` class.
///
/// ```no_run
/// # use kubuno::prelude::*;
/// # fn main_view() -> Form { Form::new() }
/// fn main() -> kubuno::Result {
///     kubuno::Application::run(main_view())
/// }
/// ```
pub struct Application;

impl Application {
    /// Opens `view` in the application's main window and runs until that window is closed — Windows
    /// Forms' `Application.Run(form)`. The window opens at the view's designed size (`DesignWidth` ×
    /// `DesignHeight` of the `.kbview`, or [`Form::client_size`]) with the view's window properties
    /// (`Title`, `StartPosition`, `FormBorderStyle`, `WindowKind`, the title bar's…), in Kubuno's
    /// chrome, at the display's DPI. In a debug build the `.kbview` is reloaded when it is saved
    /// (hot reload). Logs, `println!`s and panics go to Visual Studio's Output window under the
    /// debugger, else to `%LOCALAPPDATA%\Kubuno\logs\<exe>.log`.
    pub fn run<V: View>(mut view: V) -> crate::Result {
        let settings = settings();
        if settings.diagnostics {
            host::diagnostics::install(&host::diagnostics::exe_name(), true);
        }
        let mut driver = Driver::new(view.form(), &view, &settings);
        let options = driver.host_options(&view, &settings, None);
        host::run_with_options(options, move |canvas, frame| driver.paint(&mut view, canvas, frame))
            .map_err(|e| crate::Error::new(format!("the window could not be created: {e}")))
    }

    /// Uses `theme` for every window: the windows opened from now on, and — live — the ones
    /// already open (their title bars, pages, dialogs and tool windows repaint in it).
    pub fn set_theme(theme: Theme) {
        // The print preview dialog is drawn by `kubuno-print` in the application's theme too.
        kubuno_print::set_dialog_theme(theme.clone());
        SETTINGS.with(|s| s.borrow_mut().theme = theme.clone());
        host::set_theme_all(theme);
    }

    /// The theme in force.
    pub fn theme() -> Theme {
        settings().theme
    }

    /// Kubuno's own title bar (the default) or Windows' (`Chrome::System`); a view's own `Chrome`
    /// property wins.
    pub fn set_chrome(chrome: Chrome) {
        SETTINGS.with(|s| s.borrow_mut().chrome = chrome);
    }

    /// Windows Forms' `EnableVisualStyles()`: Kubuno's look — its title bar and the theme's
    /// colours — which is the default; kept for familiarity.
    pub fn enable_visual_styles() {
        Self::set_chrome(Chrome::Kubuno);
    }

    /// Whether a view's `.kbview` is reloaded when saved while the application runs (on by default
    /// in debug builds, off in release builds, which use the view embedded at compile time).
    pub fn set_hot_reload(on: bool) {
        SETTINGS.with(|s| s.borrow_mut().hot_reload = on);
    }

    /// Whether logs and panics go to the debugger or the log file (on by default); off for an
    /// application that installs its own `tracing` subscriber.
    pub fn set_diagnostics(on: bool) {
        SETTINGS.with(|s| s.borrow_mut().diagnostics = on);
    }

    /// Adds a filter every window message of the application goes through before it is dispatched
    /// (Windows Forms' `Application.AddMessageFilter`): return `true` to eat the message.
    pub fn add_message_filter(filter: impl FnMut(&host::MessageArgs) -> bool + 'static) {
        host::add_message_filter(filter);
    }

    /// Removes every message filter.
    pub fn clear_message_filters() {
        host::clear_message_filters();
    }

    /// The forms open in a window on this thread.
    pub fn open_forms() -> Vec<Form> {
        OPEN.with(|o| o.borrow().clone())
    }

    /// Closes every window (each form's `FormClosing` may cancel its own) — Windows Forms'
    /// `Application.Exit()`.
    pub fn exit() {
        for form in Self::open_forms() {
            form.shared.close_request.set(Some(CloseReason::ApplicationExitCall));
            form.shared.changed.set(true);
        }
        host::request_repaint_after(1);
    }
}

/// One window drawn inside a form (an MDI document, an in-window dialog).
struct Inner {
    form: Form,
    paint: InnerPaint,
    /// The whole window (band included), in the parent's client DIP.
    rect: Rect,
    /// The rectangle to go back to when un-maximised / restored.
    restore: Option<Rect>,
    minimized: bool,
    maximized: bool,
    modal: bool,
    on_closed: Option<Box<dyn FnOnce(DialogResult)>>,
    /// It has painted once (its form is open from then on).
    started: bool,
}

/// What the pointer is doing to an inner window.
#[derive(Clone, Copy)]
enum InnerGesture {
    /// Dragging it by its band, grabbed at `(dx, dy)` from its corner.
    Move { form: usize, dx: f32, dy: f32 },
    /// Resizing it by its grip.
    Resize { form: usize, start: Rect, mx: f32, my: f32 },
    /// A caption button held down.
    Button { form: usize, part: wc::Part },
}

/// One window's view: its runtime, the hot reload of its `.kbview`, and the windows drawn inside it.
pub(crate) struct Driver {
    form: Form,
    runtime: Runtime,
    /// The `.kbview` watched for hot reload, and its last modification time.
    watch: Option<(PathBuf, Option<SystemTime>)>,
    closed: bool,
    /// Drawn inside another window: it does not register as that window's form.
    inner: bool,
    children: Vec<Inner>,
    gesture: Option<InnerGesture>,
    prev_down: bool,
    /// When the window first painted (a splash screen closes itself after `SplashDuration`).
    opened_at: Option<u64>,
    /// The window had the focus last frame (a flyout closes when it loses it).
    was_focused: bool,
}

impl Driver {
    pub(crate) fn new<V: View>(form: &Form, view: &V, settings: &Settings) -> Self {
        Self::with_runtime(form, view, settings, Runtime::new(), false)
    }

    fn with_runtime<V: View>(form: &Form, view: &V, settings: &Settings, mut runtime: Runtime, inner: bool) -> Self {
        let source = form.shared.source.borrow().clone();
        let watch = source.as_ref().and_then(|s| s.path.clone()).map(PathBuf::from).filter(|p| settings.hot_reload && p.is_file());
        if let Some(dir) = source.as_ref().and_then(|s| s.path.as_ref()).and_then(|p| std::path::Path::new(p).parent().map(std::path::Path::to_path_buf)) {
            runtime.set_base_dir(Some(dir));
        }
        let mut driver = Self {
            form: form.clone(),
            runtime,
            watch: watch.map(|p| (p, None)),
            closed: false,
            inner,
            children: Vec::new(),
            gesture: None,
            prev_down: false,
            opened_at: None,
            was_focused: false,
        };
        driver.poll_file();
        driver.recompose();
        let _ = view;
        driver
    }

    /// The window options of the view: its designed size and window properties.
    pub(crate) fn host_options<V: View>(&self, view: &V, settings: &Settings, owner: Option<isize>) -> HostOptions {
        let (w, h) = self.runtime.design_size().unwrap_or_else(|| self.form.get_client_size());
        let title = Some(self.form.get_text()).filter(|t| !t.is_empty()).unwrap_or_else(host::diagnostics::exe_name);
        let mut options = HostOptions::new(&title, w.round().max(1.0) as u32, h.round().max(1.0) as u32, settings.theme.clone());
        options.chrome = self.runtime.form_spec().and_then(|s| s.chrome).unwrap_or(settings.chrome);
        options.client_size = true;
        options.fit_work_area = true;
        options.diagnostics = settings.diagnostics;
        options.form = self.runtime.form_options_in(view, &settings.theme).unwrap_or_default();
        // `start_hidden`, or a `Hide()` before the window ever opened: it opens hidden (and still
        // gets `Load`, from a frame the host runs off screen).
        options.start_hidden = self.form.shared.start_hidden.get() || self.form.shared.visibility.get() == Some(false);
        // `Owner`: an owned window stays above its owner and is minimised with it.
        let form_owner = self.form.owner().and_then(|o| o.handle());
        if let Some(o) = owner {
            options.owner = Some(o);
            options.modal = true;
            // Dialogs open centred on their owner unless the view says otherwise.
            if options.form.start_position == StartPosition::WindowsDefaultLocation {
                options.form.start_position = StartPosition::CenterParent;
            }
        } else if form_owner.is_some() {
            options.owner = form_owner;
        }
        options
    }

    /// Reads the `.kbview` again when it was saved since (hot reload).
    fn poll_file(&mut self) {
        let Some((path, last)) = &mut self.watch else { return };
        let modified = std::fs::metadata(&*path).and_then(|m| m.modified()).ok();
        if modified.is_none() || modified == *last {
            return;
        }
        let first = last.is_none();
        *last = modified;
        let Ok(text) = std::fs::read_to_string(&*path) else { return };
        let mut source = self.form.shared.source.borrow_mut();
        if let Some(s) = source.as_mut() {
            if s.text != text {
                if !first {
                    tracing::info!("{} changed: reloading the view", s.display);
                }
                s.text = text;
                self.form.shared.dirty.set(true);
            }
        }
    }

    /// Composes the view again and hands it to the runtime.
    fn recompose(&mut self) {
        let composition = compose::compose(&self.form);
        if self.runtime.reload_from_text(&composition.text) {
            return;
        }
        // Report the errors against the view as written (the composed text is not the file).
        let view_name = self.form.shared.source.borrow().as_ref().map(|s| s.display.clone()).unwrap_or_else(|| "the form".to_string());
        let original = self.form.shared.source.borrow().as_ref().map(|s| s.text.clone());
        let diagnostics = match (&original, composition.verbatim) {
            (Some(text), false) => kubuno_views::compile::compile_in(text, None).err().unwrap_or_else(|| self.runtime.diagnostics().to_vec()),
            _ => self.runtime.diagnostics().to_vec(),
        };
        for d in diagnostics.iter().take(10) {
            tracing::error!("{view_name}, line {}, column {}: {}", d.line, d.column, d.message);
        }
    }

    /// Writes what code set on properties bound to the view's own data, and reads their values back.
    fn sync_user_bindings<V: View>(&self, view: &mut V) {
        let store = self.form.shared.store.borrow().clone();
        for control in store {
            let writes = std::mem::take(&mut *control.0.pending_writes.borrow_mut());
            let bound = control.0.user_bound.borrow().clone();
            for (prop, value) in writes {
                if let Some(path) = bound.get(&prop) {
                    view.set(path, value);
                }
            }
            for (prop, path) in bound {
                if let Some(value) = view.get(&path) {
                    control.0.props.borrow_mut().insert(prop, value);
                }
            }
        }
    }

    /// One frame of the window.
    pub(crate) fn paint<V: View>(&mut self, view: &mut V, canvas: &dyn ControlCanvas, frame: &Frame) {
        let shared = self.form.shared.clone();
        if !shared.open.get() && !self.closed {
            shared.open.set(true);
            shared.hwnd.set(host::main_window().map_or(0, |h| h.0 as isize));
            *shared.dispatcher.borrow_mut() = Some(std::rc::Rc::new(self.runtime.dispatcher::<V>()));
            *shared.scope.borrow_mut() = Some(self.runtime.components());
            OPEN.with(|o| o.borrow_mut().push(self.form.clone()));
            self.opened_at = Some(host::now_ms());
        }
        if !self.inner {
            // `Form::on_message`: installed on the window, from inside one of its frames.
            for (msg, handler) in shared.message_handlers.borrow_mut().drain(..) {
                host::on_message_value(msg, handler);
            }
        }
        self.poll_file();
        if shared.dirty.get() {
            self.recompose();
        }
        self.sync_user_bindings(view);

        // The windows drawn inside this one: the page under them sees no pointer where they are.
        self.adopt_inner_requests();
        let blocked = self.children.iter().any(|c| c.modal) || self.gesture.is_some();
        let covered = !frame.pointer_outside() && self.children.iter().any(|c| c.rect.contains(frame.mouse.0, frame.mouse.1));
        let mut page_frame = *frame;
        if blocked || covered {
            page_frame.mouse = (POINTER_AWAY, POINTER_AWAY);
            page_frame.mouse_down = false;
            page_frame.right_down = false;
            page_frame.middle_down = false;
            page_frame.wheel = (0.0, 0.0);
        }

        let body = Rect::new(0.0, frame.chrome_top, frame.size.0, frame.size.1.max(frame.chrome_top));
        shared.live_size.set(Some((body.right - body.left, body.bottom - body.top)));
        // A window's content is the web `FloatingWindow`'s `--kb-window-content`: the layer surface
        // (white in the light theme), not the page ground of an application shell.
        canvas.fill_rect(&body, &canvas.theme().layer_background);
        if self.runtime.has_view() {
            let events = self.runtime.frame_model(canvas, &page_frame, view, body);
            for event in &events {
                tracing::debug!("event: {event:?}");
            }
        } else {
            let theme = canvas.theme();
            let format = &canvas.formats().body;
            let text = Rect::new(body.left + 24.0, body.top + 24.0, body.right, body.bottom);
            let message = match self.runtime.diagnostics().first() {
                Some(d) => format!("The view does not compile (line {}): {}", d.line, d.message),
                None => "Loading the view…".to_string(),
            };
            canvas.text(&message, &text, format, &theme.text_secondary, false);
        }
        self.paint_inner(canvas, frame, body);

        self.window_kind_behaviour(frame);
        if let Some(reason) = shared.close_request.take() {
            self.runtime.close(reason);
        }
        if shared.changed.replace(false) || shared.dirty.get() {
            host::request_repaint_after(1);
        }
        if self.watch.is_some() {
            host::request_repaint_after(500);
        }
        if self.runtime.is_closed() && !self.closed {
            self.closed = true;
            shared.open.set(false);
            shared.hwnd.set(0);
            shared.dispatcher.borrow_mut().take();
            shared.scope.borrow_mut().take();
            OPEN.with(|o| o.borrow_mut().retain(|f| *f != self.form));
        }
    }

    /// What the view's `WindowKind` does by itself: a splash screen closes after its duration, a
    /// flyout when it loses the focus.
    fn window_kind_behaviour(&mut self, frame: &Frame) {
        if self.inner {
            return;
        }
        let Some(spec) = self.runtime.form_spec() else { return };
        match spec.kind {
            WindowKind::Splash if spec.splash_duration > 0.0 => {
                let elapsed = self.opened_at.map_or(0, |t| host::now_ms().saturating_sub(t));
                let duration = spec.splash_duration as u64;
                if elapsed >= duration {
                    self.form.close();
                } else {
                    host::request_wake_after(u32::try_from(duration - elapsed).unwrap_or(u32::MAX).max(1));
                }
            }
            WindowKind::Flyout if self.was_focused && !frame.window_focused => self.form.close(),
            _ => {}
        }
        self.was_focused = frame.window_focused;
    }

    /// Takes the windows asked to open inside this one ([`InnerRequest`]).
    fn adopt_inner_requests(&mut self) {
        let requests: Vec<InnerRequest> = self.form.shared.pending_inner.borrow_mut().drain(..).collect();
        if requests.is_empty() {
            return;
        }
        let settings = settings();
        for r in requests {
            let band = wc::TITLEBAR_HEIGHT;
            let n = self.children.len() as f32;
            // Cascaded like Windows' MDI documents; an in-window dialog centred (placed at paint).
            let rect = Rect::new(24.0 + 28.0 * n, 24.0 + 28.0 * n, 24.0 + 28.0 * n + r.size.0, 24.0 + 28.0 * n + band + r.size.1);
            let paint = (r.make)(&settings);
            self.children.push(Inner {
                form: r.form.clone(),
                paint,
                rect,
                restore: None,
                minimized: false,
                maximized: false,
                modal: r.modal,
                on_closed: r.on_closed,
                started: false,
            });
            self.form.shared.inner.borrow_mut().push(r.form);
            self.raise_mdi_child_activate();
        }
    }

    fn raise_mdi_child_activate(&mut self) {
        self.runtime.queue_window_event(host::WindowEvent::MdiChildActivate);
        host::request_repaint_after(1);
    }

    /// Paints and drives the windows drawn inside this one (MDI documents, in-window dialogs), in
    /// the in-window `FloatingWindow` look (`kubuno_controls::window_chrome`, the web's).
    fn paint_inner(&mut self, canvas: &dyn ControlCanvas, frame: &Frame, area: Rect) {
        // Z-order follows the form's list (`Form::activate` reorders it).
        let order: Vec<Form> = self.form.shared.inner.borrow().clone();
        self.children.sort_by_key(|c| order.iter().position(|f| *f == c.form).unwrap_or(usize::MAX));
        if let Some(layout) = self.form.shared.layout_mdi.take() {
            self.layout_mdi(layout, area);
        }
        let down = frame.mouse_down;
        let pressed = down && !self.prev_down;
        let released = !down && self.prev_down;
        self.prev_down = down;
        let (mx, my) = frame.mouse;
        let band = wc::TITLEBAR_HEIGHT;
        let theme = canvas.theme().clone();
        let top = self.children.len().checked_sub(1);
        let any_modal = self.children.iter().any(|c| c.modal);

        // The pointer: a gesture in progress, then a press on the topmost window under it.
        match self.gesture {
            Some(InnerGesture::Move { form, dx, dy }) if down => {
                if let Some(c) = self.children.get_mut(form) {
                    let (w, h) = (c.rect.right - c.rect.left, c.rect.bottom - c.rect.top);
                    let left = (mx - dx).clamp(area.left - w + 60.0, area.right - 60.0);
                    let top_y = (my - dy).clamp(area.top, area.bottom - band);
                    c.rect = Rect::new(left, top_y, left + w, top_y + h);
                    c.maximized = false;
                }
            }
            Some(InnerGesture::Resize { form, start, mx: sx, my: sy }) if down => {
                if let Some(c) = self.children.get_mut(form) {
                    c.rect = Rect::new(start.left, start.top, (start.right + mx - sx).max(start.left + 200.0), (start.bottom + my - sy).max(start.top + band + 60.0));
                }
            }
            Some(InnerGesture::Button { form, part }) if released => {
                let hit = self.children.get(form).and_then(|c| inner_layout(c, band).hit(mx, my));
                if hit == Some(part) {
                    self.caption_action(form, part, area);
                }
                self.gesture = None;
            }
            Some(_) if !down => self.gesture = None,
            _ => {}
        }
        if pressed && self.gesture.is_none() && !frame.pointer_outside() {
            let under = (0..self.children.len()).rev().find(|&i| self.children[i].rect.contains(mx, my));
            let target = if any_modal { under.filter(|&i| self.children[i].modal) } else { under };
            if let Some(i) = target {
                let i = self.bring_to_front(i);
                let c = &self.children[i];
                let l = inner_layout(c, band);
                if let Some(part) = l.hit(mx, my) {
                    self.gesture = Some(InnerGesture::Button { form: i, part });
                } else if l.in_band(mx, my) {
                    if frame.click_count >= 2 && !c.modal {
                        self.caption_action(i, wc::Part::Maximize, area);
                    } else if !c.maximized {
                        self.gesture = Some(InnerGesture::Move { form: i, dx: mx - c.rect.left, dy: my - c.rect.top });
                    }
                } else if !c.modal && !c.minimized && wc::grip_rect(kubuno_controls::host::frame::grip_bounds(c.rect, inner_radius(c))).contains(mx, my) {
                    self.gesture = Some(InnerGesture::Resize { form: i, start: c.rect, mx, my });
                }
            }
        }

        // Painting, bottom first.
        let count = self.children.len();
        for i in 0..count {
            let active = Some(i) == top;
            if self.children[i].modal {
                // The web's backdrop (`bg-black/30`) over the page, under the dialog.
                canvas.fill_rect(&Rect::new(0.0, area.top, frame.size.0, frame.size.1), &theme.dialog_scrim);
                let c = &mut self.children[i];
                if !c.started {
                    // Centred like the web's `FloatingWindow` (`top: 33%`).
                    let (w, h) = (c.rect.right - c.rect.left, c.rect.bottom - c.rect.top);
                    let left = area.left + ((area.right - area.left) - w).max(0.0) / 2.0;
                    let top_y = area.top + ((area.bottom - area.top) - h).max(0.0) * 0.33;
                    c.rect = Rect::new(left, top_y, left + w, top_y + h);
                }
            }
            let c = &mut self.children[i];
            if !c.started && !c.modal {
                // Cascaded from the top-left of the client area (below the window's band).
                c.rect = Rect::new(c.rect.left + area.left, c.rect.top + area.top, c.rect.right + area.left, c.rect.bottom + area.top);
            }
            let l = inner_layout(c, band);
            let radius = inner_radius(c);
            wc::paint_frame(canvas, c.rect, radius, None);
            // Nothing the window holds paints past its rounded corners.
            canvas.push_clip_rounded(&c.rect, radius);
            let style = inner_style(c);
            wc::paint_band_rounded(canvas, &style, &l, radius);
            let client = Rect::new(c.rect.left, c.rect.top + band, c.rect.right, c.rect.bottom);
            if !c.minimized && client.bottom > client.top {
                let over = !frame.pointer_outside() && c.rect.contains(mx, my) && l.hit(mx, my).is_none();
                let live = active && self.gesture.is_none();
                let mut f = *frame;
                f.size = (client.right - client.left, client.bottom - client.top);
                f.chrome_top = 0.0;
                f.client_origin = (frame.client_origin.0 + client.left, frame.client_origin.1 + client.top);
                if live && (over || down) {
                    f.mouse = (mx - client.left, my - client.top);
                } else {
                    f.mouse = (POINTER_AWAY, POINTER_AWAY);
                    f.mouse_down = false;
                    f.right_down = false;
                    f.middle_down = false;
                }
                if !(live && over) {
                    f.wheel = (0.0, 0.0);
                }
                f.window_focused = frame.window_focused && active;
                // The view's own title-bar controls go to this window's band, above its page.
                kubuno_views::window::set_design_chrome(Some(kubuno_views::window::DesignChrome {
                    style: style.clone(),
                    bounds: Rect::new(0.0, -band, f.size.0, f.size.1),
                    has_icon: false,
                    buttons: inner_buttons(c),
                }));
                canvas.push_clip(&c.rect);
                canvas.push_offset(client.left, client.top);
                (c.paint)(canvas, &f);
                canvas.pop_offset();
                canvas.pop_clip();
                kubuno_views::window::set_design_chrome(None);
                c.started = true;
            }
            let hot = if active || !any_modal { l.hit(mx, my) } else { None };
            let pressed_part = match self.gesture {
                Some(InnerGesture::Button { form, part }) if form == i => Some(part),
                _ => None,
            };
            let state = wc::ChromeState { hot, pressed: pressed_part, maximized: c.maximized };
            let title = c.form.get_text();
            let icon = c.form.root().string("Icon");
            let glyph = drive_app_controls_icon(&icon);
            wc::paint_caption(canvas, &style, &l, &title, glyph.map_or(wc::ChromeIcon::None, wc::ChromeIcon::Glyph), state);
            if !c.modal && !c.minimized && !c.maximized {
                wc::paint_grip(canvas, kubuno_controls::host::frame::grip_bounds(c.rect, radius), false);
            }
            canvas.pop_clip_rounded();
        }

        // The windows that closed (their `FormClosed` ran): gone, their result handed back.
        let mut closed = false;
        let mut i = 0;
        while i < self.children.len() {
            if self.children[i].started && !self.children[i].form.shared.open.get() {
                let mut c = self.children.remove(i);
                self.form.shared.inner.borrow_mut().retain(|f| *f != c.form);
                if let Some(done) = c.on_closed.take() {
                    let result = match c.form.dialog_result() {
                        DialogResult::None => DialogResult::Cancel,
                        r => r,
                    };
                    done(result);
                }
                closed = true;
                self.gesture = None;
            } else {
                i += 1;
            }
        }
        if closed {
            self.raise_mdi_child_activate();
            host::request_repaint_after(1);
        }
        if self.gesture.is_some() {
            host::request_repaint_after(16);
        }
    }

    /// Moves window `i` to the top; returns its new index.
    fn bring_to_front(&mut self, i: usize) -> usize {
        let last = self.children.len() - 1;
        if i == last {
            return i;
        }
        let c = self.children.remove(i);
        let form = c.form.clone();
        self.children.push(c);
        {
            let mut order = self.form.shared.inner.borrow_mut();
            order.retain(|f| *f != form);
            order.push(form);
        }
        self.raise_mdi_child_activate();
        last
    }

    /// A click on one of an inner window's caption buttons.
    fn caption_action(&mut self, i: usize, part: wc::Part, area: Rect) {
        let band = wc::TITLEBAR_HEIGHT;
        let Some(c) = self.children.get_mut(i) else { return };
        match part {
            wc::Part::Close => c.form.close(),
            wc::Part::Maximize => {
                if c.maximized {
                    if let Some(r) = c.restore.take() {
                        c.rect = r;
                    }
                    c.maximized = false;
                } else {
                    c.restore.get_or_insert(c.rect);
                    c.rect = area;
                    c.maximized = true;
                    c.minimized = false;
                }
            }
            wc::Part::Minimize => {
                if c.minimized {
                    if let Some(r) = c.restore.take() {
                        c.rect = r;
                    }
                    c.minimized = false;
                } else {
                    c.restore.get_or_insert(c.rect);
                    c.minimized = true;
                    c.maximized = false;
                    c.rect = Rect::new(c.rect.left, c.rect.top, c.rect.left + 240.0, c.rect.top + band);
                }
            }
            _ => {}
        }
        host::request_repaint_after(1);
    }

    /// `LayoutMdi`: arranges the documents in `area`.
    fn layout_mdi(&mut self, layout: MdiLayout, area: Rect) {
        let band = wc::TITLEBAR_HEIGHT;
        let docs: Vec<usize> = (0..self.children.len()).filter(|&i| !self.children[i].modal).collect();
        let n = docs.len().max(1) as f32;
        let (w, h) = (area.right - area.left, area.bottom - area.top);
        for (k, &i) in docs.iter().enumerate() {
            let k = k as f32;
            let c = &mut self.children[i];
            if layout != MdiLayout::ArrangeIcons {
                c.minimized = false;
                c.maximized = false;
                c.restore = None;
            }
            c.rect = match layout {
                MdiLayout::Cascade => {
                    let off = 28.0 * k;
                    Rect::new(area.left + off, area.top + off, area.left + off + w * 0.6, area.top + off + h * 0.6)
                }
                MdiLayout::TileHorizontal => Rect::new(area.left, area.top + h * k / n, area.right, area.top + h * (k + 1.0) / n),
                MdiLayout::TileVertical => Rect::new(area.left + w * k / n, area.top, area.left + w * (k + 1.0) / n, area.bottom),
                MdiLayout::ArrangeIcons if c.minimized => {
                    let x = area.left + 244.0 * k;
                    Rect::new(x, area.bottom - band, x + 240.0, area.bottom)
                }
                MdiLayout::ArrangeIcons => c.rect,
            };
        }
    }
}

/// A Lucide glyph named by a form's `Icon` (not a file).
fn drive_app_controls_icon(icon: &str) -> Option<&'static str> {
    wc::icon_glyph(icon)
}

/// The band of an inner window: the web `FloatingWindow`'s; a tool window's slim one.
fn inner_style(c: &Inner) -> wc::ChromeStyle {
    let kind = c.form.root().string("WindowKind");
    wc::ChromeStyle { tool: kind == "ToolWindow", subtitle: c.form.root().string("Subtitle"), ..wc::ChromeStyle::default() }
}

/// The radius of an inner window's corners, in DIP: its view's `CornerRadius`, else its
/// `CornerPreference`'s (8 by default, a desktop window's); square while it is maximised in its
/// parent, as Windows squares a maximised window.
fn inner_radius(c: &Inner) -> f32 {
    use kubuno_controls::host::{form, CornerPreference};
    if c.maximized {
        return 0.0;
    }
    let root = c.form.root();
    form::parse_corner_radius(&root.string("CornerRadius")).unwrap_or_else(|| {
        let preference = match root.string("CornerPreference").as_str() {
            "Round" => CornerPreference::Round,
            "RoundSmall" => CornerPreference::RoundSmall,
            "DoNotRound" => CornerPreference::DoNotRound,
            _ => CornerPreference::Default,
        };
        preference.radius_for(root.string("FormBorderStyle") != "None")
    })
}

/// The caption buttons of an inner window: an MDI document's three, an in-window dialog's close.
fn inner_buttons(c: &Inner) -> wc::SystemButtons {
    if c.modal {
        wc::SystemButtons::CLOSE_ONLY
    } else {
        wc::SystemButtons::default()
    }
}

fn inner_layout(c: &Inner, _band: f32) -> wc::ChromeLayout {
    let icon = drive_app_controls_icon(&c.form.root().string("Icon")).is_some();
    wc::layout(&inner_style(c), c.rect, icon, inner_buttons(c), wc::SlotWidths::default())
}

/// `View::show`: a modeless window on the running loop — or, for a form with an MDI parent, a
/// document inside the parent's window.
pub(crate) fn show<V: View>(mut view: V) {
    let settings = settings();
    let form = view.form().clone();
    if let Some(parent) = form.mdi_parent() {
        open_inner(&parent, view, false, None);
        return;
    }
    let mut driver = Driver::new(&form, &view, &settings);
    let options = driver.host_options(&view, &settings, None);
    if let Err(e) = host::open_window(options, move |canvas, frame| driver.paint(&mut view, canvas, frame)) {
        tracing::error!("the window could not be created: {e}");
    }
}

/// Opens `view` inside `parent`'s window at its next frame: an MDI document, or (`modal`) an
/// in-window dialog that veils the page until it closes, then calls `on_closed`.
pub(crate) fn open_inner<V: View>(parent: &Form, view: V, modal: bool, on_closed: Option<Box<dyn FnOnce(DialogResult)>>) {
    let form = view.form().clone();
    let size = form.get_client_size();
    let make_form = form.clone();
    let make = Box::new(move |settings: &Settings| -> InnerPaint {
        let mut view = view;
        let mut driver = Driver::with_runtime(&make_form, &view, settings, Runtime::new_inner(), true);
        Box::new(move |canvas: &dyn ControlCanvas, frame: &Frame| driver.paint(&mut view, canvas, frame))
    });
    if modal {
        form.shared.modal.set(true);
    }
    parent.shared.pending_inner.borrow_mut().push(InnerRequest { form, modal, size, make, on_closed });
    parent.shared.changed.set(true);
    host::request_repaint_after(1);
}

/// `View::show_dialog`: a modal window, owned by `owner` (else by the window whose frame is
/// running), with a nested loop until it closes.
pub(crate) fn show_dialog<V: View>(view: &mut V, owner: Option<isize>) -> DialogResult {
    let settings = settings();
    let form = view.form().clone();
    let owner = owner.or_else(|| host::main_window().map(|h| h.0 as isize)).unwrap_or(0);
    form.shared.modal.set(true);
    form.shared.dialog_result.set(DialogResult::None);
    let mut driver = Driver::new(&form, &*view, &settings);
    let mut options = driver.host_options(&*view, &settings, Some(owner));
    if owner == 0 {
        options.owner = None;
    }
    let result = host::run_scoped(options, |canvas, frame| driver.paint(&mut *view, canvas, frame));
    form.shared.modal.set(false);
    if let Err(e) = result {
        tracing::error!("the dialog could not be created: {e}");
        return DialogResult::None;
    }
    // Closed from its caption (or Alt+F4, Escape without a CancelButton): Cancel, as in Windows Forms.
    match form.shared.dialog_result.get() {
        DialogResult::None => {
            form.shared.dialog_result.set(DialogResult::Cancel);
            DialogResult::Cancel
        }
        r => r,
    }
}
