//! The input router (`vskubuno/docs/EVENTS.md` §3, work package EVT-2): turns the
//! host's frame-granular input (`kubuno_desktop_controls::host::Frame`'s latched buttons,
//! pointer and wheel, the key/text queue) and the focus ring's recorded moves
//! (`kubuno_desktop_ui::FocusRing::take_changes`) into the ordered WinForms event sequences,
//! raised directly on the element concerned:
//!
//! - **Click:** MouseDown → Click → MouseClick → MouseUp. Click needs a release inside
//!   after a press inside; keyboard activation (Space/Enter on a button) raises Click alone.
//! - **Double-click:** MouseDown, Click, MouseClick, MouseUp, MouseDown, DoubleClick,
//!   MouseDoubleClick, MouseUp; an element without standard double-click (a button)
//!   raises a second Click instead.
//! - **Hover:** MouseEnter → MouseMove* → MouseHover (once, after [`HOVER_DELAY_MS`]
//!   stationary) → MouseLeave.
//! - **Keys** (to the focused element): KeyDown → KeyPress (per character, unless the
//!   KeyDown suppressed it) → KeyUp.
//! - **Focus by keyboard or code:** Enter → GotFocus on the new element; then on the old
//!   one Leave → Validating → Validated → LostFocus. **By the pointer:** Enter → GotFocus;
//!   old: LostFocus → Leave → Validating → Validated. Enter/Leave are raised on the
//!   containers too (focus-within, outermost first for Enter, innermost first for Leave).
//!   A Validating cancelled by a handler keeps the focus on the old element.
//! - **View:** Load → Activated → Shown on the first frame, Activated/Deactivate on the
//!   window's focus edges, Move/LocationChanged when the window moves on screen; closing (EVT-6,
//!   driven by `crate::runtime::Runtime`): FormClosing (cancelable) → FormClosed → Deactivate.
//! - **Layout:** Resize → SizeChanged, Move → LocationChanged when an element's painted
//!   bounds change between two frames.
//!
//! ## How it runs
//!
//! Like the focus ring, it routes input against the PREVIOUS frame's geometry: every
//! compiled element is wrapped in `crate::design::DesignSlot`, whose paint registers the
//! element ([`SlotEvents`]) and its bounds here, in paint order. At the start of a frame
//! ([`InputRouter::begin_frame`], before anything paints) the router hit-tests the pointer
//! against that list (the deepest element wins, like `LayoutMap::hit_test`), raises
//! everything that precedes a control's own reaction (focus, enter/leave, move, hover,
//! MouseDown, wheel, keys), and queues what must follow it; the frame then paints (a
//! `<Button>` raises its own Click there); [`InputRouter::end_frame`] raises the queued
//! MouseClick/MouseUp and the layout events. A pressed element captures the mouse until
//! the button is released (WinForms' `Capture`).
//!
//! An event is dispatched only to the handler its element names (`OnMouseDown="…"`, or
//! an older alias), through [`HandlerTable::dispatch_args`] — the typed handler when there
//! is one, else the legacy `handlers!` entry with the args' legacy value — and it is
//! reported in the frame's returned events as [`ViewEventKind::Other`]. Elements that do
//! not ask for an event cost nothing but the hit-test.
//!
//! `KeyPreview` (the view root gets the keys first) and `CausesValidation` (an element that does not
//! validate the one losing the focus) are honoured (`EVENTS.md` §16). Not built here: wheel bubbling,
//! drag and drop beyond a file drop (`crate::runtime` raises `OnDragDrop`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use kubuno_desktop_controls::host::{vk, Frame, InputEvent};
use kubuno_desktop_ui::{FocusCause, FocusChange, FocusId, FocusRing, Rect};

use super::{
    CancelEventArgs, CloseReason, ElementRef, EmptyEventArgs, EventArgs, FormClosedEventArgs, FormClosingEventArgs, Handled, Key, KeyEventArgs, KeyPressEventArgs, MouseButton,
    MouseEventArgs,
};
use crate::binding::{HandlerTable, ViewModel};
use crate::component::{Component, Control, EventCx, Keys, Message, RaiseSink};
use crate::node::{ViewEvent, ViewEventKind};
use crate::registry::ComponentMeta;

/// How long the pointer must rest on an element before MouseHover (WinForms'
/// `SystemInformation.MouseHoverTime` default).
pub const HOVER_DELAY_MS: u64 = 400;

/// The elements whose Space/Enter raises Click (WinForms `ButtonBase`, `LinkLabel`).
const KEYBOARD_CLICK_ELEMENTS: &[&str] = &["Button", "IconButton", "LinkLabel"];

/// What the router knows about one compiled element: its identity (the sender of its
/// events), the handler each of its `On*` attributes names, and how it clicks. Built once
/// per compile by `crate::compile::build_node`, shared (`Rc`) with its `DesignSlot`.
#[derive(Debug, Clone)]
pub struct SlotEvents {
    /// The stable element id (`DESIGNER.md` §8): `""` for the root, `"0.2"`…
    pub id: String,
    /// The element name (`"Button"`).
    pub element: &'static str,
    /// Its `x:Name`.
    pub name: Option<String>,
    pub focus_id: Option<FocusId>,
    /// Canonical event attribute (`"OnMouseDown"`) → handler name.
    handlers: Vec<(&'static str, String)>,
    /// The element's own node raises Click on a mouse click (a `<Button>`): the router
    /// then never synthesizes one, and a second click is a second Click, not a DoubleClick.
    pub native_click: bool,
    /// Space/Enter on the focused element raises Click.
    pub keyboard_click: bool,
    /// A second click within the double-click time raises DoubleClick/MouseDoubleClick.
    pub standard_double_click: bool,
    /// Its non-event XML attributes as written, the sender's properties (`ElementRef::attributes`).
    pub attributes: Vec<(String, String)>,
    /// Report every event the element's control raises in the frame's returned events, not only
    /// those a handler is named for (the elements of a `crate::component::ControlHost`).
    pub(crate) report_all: bool,
    /// `CausesValidation`: moving the focus to this element validates the element that had it
    /// (WinForms' default); `false` skips Validating/Validated (a Cancel button).
    pub causes_validation: bool,
}

impl SlotEvents {
    /// An element with no handler (tests, or before any `On*` is read).
    pub fn new(id: impl Into<String>, element: &'static str) -> Self {
        Self {
            id: id.into(),
            element,
            name: None,
            focus_id: None,
            handlers: Vec::new(),
            native_click: false,
            keyboard_click: false,
            standard_double_click: true,
            attributes: Vec::new(),
            report_all: false,
            causes_validation: true,
        }
    }

    /// Reads a parsed element's `x:Name` and every `On*` attribute (canonical names and
    /// older aliases alike, resolved against `meta`; the view events of
    /// `crate::registry::VIEW_EVENTS` too when `root`).
    pub fn from_element(element: &crate::ast::Element, meta: &'static ComponentMeta, root: bool) -> Self {
        let mut slot = Self::new(crate::compile::scoped_id(element.stable_id()), meta.name);
        slot.name = element.attribute("x:Name").and_then(|a| a.value()).filter(|s| !s.is_empty());
        slot.focus_id = slot.name.as_deref().map(FocusId::of).map(crate::compile::scoped_focus);
        slot.native_click = meta.events.iter().any(|e| e.name == "OnClick") && !crate::registry::project::project_info(meta.name).is_some_and(|i| i.routed_click);
        slot.keyboard_click = KEYBOARD_CLICK_ELEMENTS.contains(&meta.name);
        slot.standard_double_click = !slot.native_click;
        slot.causes_validation = element.attribute("CausesValidation").and_then(|a| a.value()).is_none_or(|v| v.trim() != "false");
        for attr in element.attributes() {
            let Some(attr_name) = attr.name() else { continue };
            if !attr_name.starts_with("On") && !attr_name.starts_with("x:") && !attr_name.starts_with("xmlns") {
                slot.attributes.push((attr_name.to_string(), attr.value().unwrap_or_default()));
                continue;
            }
            if !attr_name.starts_with("On") {
                continue;
            }
            let Some(handler) = attr.value().filter(|v| !v.is_empty()) else { continue };
            let event = meta.event(&attr_name).or_else(|| if root { crate::registry::view_event(&attr_name) } else { None });
            if let Some(event) = event {
                // The canonical attribute wins over an alias written on the same element.
                if !slot.handlers.iter().any(|(e, _)| *e == event.name) || attr_name == event.name {
                    slot.handlers.retain(|(e, _)| *e != event.name);
                    slot.handlers.push((event.name, handler));
                }
            }
        }
        slot
    }

    /// Adds (or replaces) the handler of `event` (its canonical attribute, `"OnMouseDown"`).
    pub fn with_handler(mut self, event: &'static str, handler: impl Into<String>) -> Self {
        self.handlers.retain(|(e, _)| *e != event);
        self.handlers.push((event, handler.into()));
        self
    }

    /// The handler the element names for `event` (its canonical attribute).
    pub fn handler(&self, event: &str) -> Option<&str> {
        self.handlers.iter().find(|(e, _)| *e == event).map(|(_, h)| h.as_str())
    }

    /// The sender of this element's events, painted at `bounds`.
    pub fn sender(&self, bounds: Rect) -> ElementRef<'_> {
        ElementRef { name: self.name.as_deref(), element: self.element, id: &self.id, bounds, focus_id: self.focus_id, attributes: &self.attributes }
    }
}

/// What a raise needs to reach the application: the view model, the handler table, and
/// the frame's returned events.
pub struct Dispatch<'a> {
    pub vm: &'a mut dyn ViewModel,
    pub handlers: &'a mut HandlerTable,
    pub events: &'a mut Vec<ViewEvent>,
}

/// Where the handlers of the elements of a NESTED view run: a user control's own view (its handlers are the user
/// control's methods, its bindings its properties) or a `<Repeater>` item (the row, then the item's user control,
/// then the page). The elements of such a view are registered with the window's router like the page's own
/// (`InputRouter::push_scope`), so a control inside a user control gets its mouse, wheel and key events; when one of
/// them raises an event, the router dispatches its handler through the scope instead of the page.
pub(crate) trait DispatchScope {
    /// Runs `f` with a dispatch whose view model is the nested view's (it may fall back to `d`'s, the page's).
    fn with_dispatch(&self, d: &mut Dispatch<'_>, f: &mut dyn FnMut(&mut Dispatch<'_>));
}

/// The scope of a user control's own view: its instance is the view model (`ViewModel` of `#[derive(UserControl)]`);
/// a handler it does not have runs on the page (`d`).
pub(crate) struct ControlScope {
    pub(crate) control: Weak<RefCell<dyn Component>>,
    /// The nested view the user control itself is in (a user control in a user control, in a `<Repeater>` item):
    /// what its own view model does not answer goes there, then to the page.
    pub(crate) parent: Option<Rc<dyn DispatchScope>>,
}

impl DispatchScope for ControlScope {
    fn with_dispatch(&self, d: &mut Dispatch<'_>, f: &mut dyn FnMut(&mut Dispatch<'_>)) {
        match &self.parent {
            Some(parent) => parent.with_dispatch(d, &mut |outer| self.own_dispatch(outer, &mut *f)),
            None => self.own_dispatch(d, f),
        }
    }
}

impl ControlScope {
    fn own_dispatch(&self, d: &mut Dispatch<'_>, f: &mut dyn FnMut(&mut Dispatch<'_>)) {
        let Some(cell) = self.control.upgrade() else { return f(d) };
        // The instance is busy (its own code is running): the page's dispatch, as before nested routing.
        let Ok(mut component) = cell.try_borrow_mut() else { return f(d) };
        match component.kubuno_view_model() {
            Some(vm) => {
                let mut fallback = ScopedVm { inner: vm, outer: &mut *d.vm };
                let mut nested = Dispatch { vm: &mut fallback, handlers: &mut *d.handlers, events: &mut *d.events };
                f(&mut nested);
            }
            None => f(d),
        }
    }
}

/// A nested view's model over the page's: reads and handlers go to `inner` first, then to `outer`.
pub(crate) struct ScopedVm<'a, 'b> {
    pub(crate) inner: &'a mut dyn ViewModel,
    pub(crate) outer: &'b mut dyn ViewModel,
}

impl ViewModel for ScopedVm<'_, '_> {
    fn get(&self, path: &str) -> Option<crate::binding::Value> {
        self.inner.get(path).or_else(|| self.outer.get(path))
    }
    fn set(&mut self, path: &str, value: crate::binding::Value) {
        if self.inner.get(path).is_some() {
            self.inner.set(path, value);
        } else {
            self.outer.set(path, value);
        }
    }
    fn dispatch_event(&mut self, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        self.inner.dispatch_event(handler, sender, args) || self.outer.dispatch_event(handler, sender, args)
    }
}

/// Raises `event` (its canonical attribute) on `slot`: dispatches the handler the element
/// names for it, reports it as [`ViewEventKind::Other`], and hands the args back so the
/// caller can read `handled`/`cancel`. A no-op (the args come back untouched) when the
/// element does not ask for this event.
pub fn raise<A: EventArgs + Clone>(d: &mut Dispatch<'_>, slot: &SlotEvents, bounds: Rect, event: &'static str, mut args: A) -> A {
    let Some(handler) = slot.handler(event) else { return args };
    let sender = slot.sender(bounds);
    d.handlers.dispatch_args(handler, d.vm, &sender, &mut args);
    d.events.push(ViewEvent {
        focus_id: slot.focus_id,
        handler: Some(handler.to_string()),
        kind: ViewEventKind::Other { name: event.strip_prefix("On").unwrap_or(event), args: Rc::new(args.clone()) },
    });
    args
}

/// The element's control (a class of `crate::component`), shared with its `DesignSlot` or
/// `ControlHost`, which owns it.
pub(crate) type ControlCell = Rc<RefCell<dyn Component>>;

/// [`raise`] for a registered element: through its nested view's scope when it has one.
fn raise_in<A: EventArgs + Clone>(d: &mut Dispatch<'_>, r: &Registered, event: &'static str, args: A) -> A {
    let args = match &r.scope {
        None => raise(d, &r.slot, r.bounds, event, args),
        Some(scope) => {
            let mut args = args;
            scope.with_dispatch(d, &mut |nested| {
                args = raise(nested, &r.slot, r.bounds, event, args.clone());
            });
            args
        }
    };
    raise_own(d, r, event, args)
}

/// A user control's own subscription to `event` (the handler its own view's root names, `<UserControl
/// OnClick="…">`: Windows Forms' `this.Click += …` in its `InitializeComponent`), run on the user control after the
/// handler of the element using it.
fn raise_own<A: EventArgs + Clone>(d: &mut Dispatch<'_>, r: &Registered, event: &'static str, args: A) -> A {
    let Some(own) = &r.own else { return args };
    if own.slot.handler(event).is_none() {
        return args;
    }
    let mut args = args;
    match &own.scope {
        Some(scope) => scope.with_dispatch(d, &mut |nested| {
            args = raise(nested, &own.slot, r.bounds, event, args.clone());
        }),
        None => args = raise(d, &own.slot, r.bounds, event, args),
    }
    args
}

/// [`raise`] through the element's control, when it has one (EVT-7a): the event is delivered
/// to the control's `on_…` method ([`Control::dispatch_event`]), whose base behaviour raises it
/// to the element's handler (this sink) then to the control's Rust subscribers. An override that
/// does not call its base suppresses the event. Without a control (or while it is borrowed), the
/// handler is dispatched directly, as before.
fn raise_to<A: EventArgs + Clone>(d: &mut Dispatch<'_>, r: &Registered, event: &'static str, mut args: A) -> A {
    let Some(cell) = r.control.as_ref().and_then(Weak::upgrade) else { return raise_in(d, r, event, args) };
    let Ok(mut component) = cell.try_borrow_mut() else { return raise_in(d, r, event, args) };
    let raised = {
        let mut sink = SlotSink { d: &mut *d, slot: &r.slot, bounds: r.bounds, raised: None, scope: r.scope.as_ref(), reached: Vec::new() };
        let mut e = EventCx::with_sink(&mut args as &mut dyn EventArgs, &mut sink);
        match component.as_control_mut() {
            Some(control) => control.dispatch_event(event, &mut e),
            None => e.raise(&*component, event),
        }
        (sink.raised, sink.reached.contains(&event))
    };
    drop(component);
    let (raised, reached) = raised;
    if reached {
        args = raise_own(d, r, event, args);
    }
    if let Some((raised_event, handler)) = raised {
        d.events.push(ViewEvent {
            focus_id: r.slot.focus_id,
            handler,
            kind: ViewEventKind::Other { name: raised_event.strip_prefix("On").unwrap_or(raised_event), args: Rc::new(args.clone()) },
        });
    }
    args
}

/// The router's [`RaiseSink`]: an event raised by a control's base behaviour reaches the handler
/// its element names (and is then reported in the frame's events).
struct SlotSink<'s, 'd> {
    d: &'s mut Dispatch<'d>,
    slot: &'s SlotEvents,
    bounds: Rect,
    /// The first event raised to the sink, and the handler it ran.
    raised: Option<(&'static str, Option<String>)>,
    /// The nested view's scope of the element (see [`DispatchScope`]).
    scope: Option<&'s Rc<dyn DispatchScope>>,
    /// The events the control's base behaviour raised (a user control's own subscriptions run after, once the
    /// control is released: [`raise_own`]).
    reached: Vec<&'static str>,
}

/// The root element of a user control's own view, merged into the element using the user control: the user
/// control IS its view's root (Windows Forms), so the pointer over its surface reaches the user control, and the
/// handlers its root names are the user control's own subscriptions to its events.
#[derive(Clone)]
struct OwnRoot {
    slot: Rc<SlotEvents>,
    /// The user control's scope (its handlers are its methods).
    scope: Option<Rc<dyn DispatchScope>>,
}

impl RaiseSink for SlotSink<'_, '_> {
    fn sender(&self) -> ElementRef<'_> {
        self.slot.sender(self.bounds)
    }

    fn raise(&mut self, event: &'static str, args: &mut dyn EventArgs) {
        let handler = self.slot.handler(event).map(str::to_string);
        if let Some(h) = &handler {
            let sender = self.slot.sender(self.bounds);
            match self.scope {
                Some(scope) => scope.with_dispatch(&mut *self.d, &mut |nested| {
                    nested.handlers.dispatch_args(h, &mut *nested.vm, &sender, &mut *args);
                }),
                None => {
                    self.d.handlers.dispatch_args(h, &mut *self.d.vm, &sender, args);
                }
            }
        }
        self.reached.push(event);
        if (handler.is_some() || self.slot.report_all) && self.raised.is_none() {
            self.raised = Some((event, handler));
        }
    }
}

/// One frame's input, as the router reads it.
pub struct FrameInput<'a> {
    pub frame: &'a Frame,
    /// A monotonic clock (`kubuno_desktop_controls::host::now_ms`), for MouseHover.
    pub now_ms: u64,
    /// The frame's unconsumed host events (`kubuno_desktop_controls::host::events`), in order.
    pub events: &'a [InputEvent],
}

/// What [`InputRouter::begin_frame`] asks of its caller.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BeginOutcome {
    /// Repaint in this many ms even without input (a pending MouseHover).
    pub repaint_after: Option<u32>,
    /// Indices into [`FrameInput::events`] a handler marked handled: consume them so the
    /// focused control does not also act on them.
    pub consumed: Vec<usize>,
}

#[derive(Clone)]
struct Registered {
    slot: Rc<SlotEvents>,
    /// As painted (content coordinates).
    bounds: Rect,
    /// In client coordinates (scroll offset applied): where the mouse event coordinates are relative to.
    client: Rect,
    /// `client` cut by the clips of the containers around it (`crate::clip`): what the pointer is tested against
    /// (a control is not hit over its clipped-away part, as in WinForms).
    hit: Rect,
    /// The element's control, when it has one (EVT-7a).
    control: Option<Weak<RefCell<dyn Component>>>,
    /// `Enabled`: a disabled element (or one inside a disabled container) takes no pointer input,
    /// and the pointer over it reaches nothing (WinForms).
    enabled: bool,
    /// The nested view it belongs to (a user control's own view, a `<Repeater>` item), if any.
    scope: Option<Rc<dyn DispatchScope>>,
    /// A user control: its own view's root, merged into it ([`InputRouter::absorb_view_root`]).
    own: Option<OwnRoot>,
}

impl Registered {
    /// An element that was not painted (its bounds unknown): the root before its first frame.
    fn detached(slot: Rc<SlotEvents>) -> Self {
        Self { slot, bounds: Rect::default(), client: Rect::default(), hit: Rect::default(), control: None, enabled: true, scope: None, own: None }
    }

    /// Runs `f` on the element's control when it has one that is not borrowed.
    fn with_control<R>(&self, f: impl FnOnce(&mut dyn Control) -> R) -> Option<R> {
        let cell = self.control.as_ref()?.upgrade()?;
        let mut component = cell.try_borrow_mut().ok()?;
        let control = component.as_control_mut()?;
        Some(f(control))
    }
}

#[derive(Debug, Clone)]
struct Capture {
    id: String,
    button: MouseButton,
    clicks: u8,
}

enum Post {
    /// A mouse event raised after the paint (Click, MouseClick, MouseUp…).
    Mouse(&'static str, MouseEventArgs),
}

/// The per-view input router: see the module doc. Owned by `crate::runtime::Runtime`
/// across frames (and hot reloads: its state is keyed by stable element id).
#[derive(Default)]
pub struct InputRouter {
    prev: Vec<Registered>,
    cur: Vec<Registered>,
    root: Option<Rc<SlotEvents>>,
    hot: Option<String>,
    capture: Option<Capture>,
    buttons: [bool; 3],
    last_mouse: Option<(f32, f32)>,
    still_since: u64,
    hover_fired: bool,
    last_bounds: HashMap<String, Rect>,
    post: Vec<(Registered, Post)>,
    loaded: bool,
    shown: bool,
    /// The window is hidden this frame (started hidden, `Hide()`): `Load` is raised regardless,
    /// `Shown` waits for the first frame the window is on screen.
    hidden: bool,
    window_active: Option<bool>,
    /// The window's client origin on screen at the last frame (the view root's Move).
    window_origin: Option<(f32, f32)>,
    /// `KeyPreview` on the view's root: it gets the key events before the focused element.
    key_preview: bool,
    /// The window events ([`kubuno_desktop_controls::host::WindowEvent`]) to raise at the next frame.
    window_events: Vec<kubuno_desktop_controls::host::WindowEvent>,
    /// The nested views being painted (innermost last): the elements registered meanwhile belong to the last.
    scopes: Vec<Rc<dyn DispatchScope>>,
}

impl InputRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the window's events (the host's [`kubuno_desktop_controls::host::take_window_events`]) for the
    /// view's own events of the same name, raised at the start of the next frame.
    pub fn queue_window_events(&mut self, events: Vec<kubuno_desktop_controls::host::WindowEvent>) {
        self.window_events.extend(events);
    }

    /// Whether the view's window is on screen this frame (read by [`Self::end_frame`]): Windows
    /// Forms raises `Load` when the form is created, visible or not, and `Shown` the first time it
    /// is actually shown — a form started hidden gets `Load` from a frame run off screen.
    pub fn set_window_visible(&mut self, visible: bool) {
        self.hidden = !visible;
    }

    /// The root element of the (re)compiled view — the sender of the view events. Load is
    /// raised once, for the first view ever set; a hot reload does not raise it again.
    pub fn set_root(&mut self, root: Rc<SlotEvents>) {
        self.root = Some(root);
    }

    /// Starts a nested view (a user control's own view, a `<Repeater>` item): the elements registered until the
    /// matching [`Self::pop_scope`] dispatch their handlers through `scope`.
    pub(crate) fn push_scope(&mut self, scope: Rc<dyn DispatchScope>) {
        self.scopes.push(scope);
    }

    /// Ends the nested view [`Self::push_scope`] started.
    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// The scope of the user control registered last (the element whose node is painting its own view now).
    pub(crate) fn control_scope_of_last(&self) -> Option<Rc<dyn DispatchScope>> {
        let control = self.cur.last()?.control.clone()?;
        Some(Rc::new(ControlScope { control, parent: self.scopes.last().cloned() }))
    }

    /// Records one element painted this frame at `bounds` (content coordinates), in paint
    /// order — called by `crate::design::DesignSlot::paint`.
    pub fn register(&mut self, slot: Rc<SlotEvents>, bounds: Rect) {
        self.register_with_control(slot, bounds, None);
    }

    /// [`Self::register`] with the element's control, through whose `on_…` methods its events
    /// are then delivered (EVT-7a).
    pub(crate) fn register_with_control(&mut self, slot: Rc<SlotEvents>, bounds: Rect, control: Option<&ControlCell>) {
        let (dx, dy) = kubuno_desktop_controls::host::content_offset();
        let client = Rect::new(bounds.left + dx, bounds.top + dy, bounds.right + dx, bounds.bottom + dy);
        let hit = crate::clip::visible_client(client);
        self.cur.push(Registered { slot, bounds, client, hit, control: control.map(Rc::downgrade), enabled: true, scope: self.scopes.last().cloned(), own: None });
    }

    /// How many elements were registered so far this frame (where a nested view's registrations start).
    pub(crate) fn registered_count(&self) -> usize {
        self.cur.len()
    }

    /// A user control finished painting its own view, whose elements were registered from `start` on (the first
    /// one its root): the root is merged into the user control's element registered just before (see
    /// [`OwnRoot`]) instead of covering it — the pointer over the user control's own surface reaches the user
    /// control (its `on_mouse_…` overrides, the `Click` of the element using it), as in Windows Forms.
    pub(crate) fn absorb_view_root(&mut self, start: usize) {
        if start == 0 || start >= self.cur.len() || self.cur[start - 1].own.is_some() {
            return;
        }
        let root = self.cur.remove(start);
        self.cur[start - 1].own = Some(OwnRoot { slot: root.slot, scope: root.scope });
    }

    /// A `<Repeater ItemTemplate="…">` item finished painting its user control's view, whose root was registered
    /// at `start`: the root is the item's user control (its `on_mouse_…` overrides get the pointer over its surface).
    pub(crate) fn attach_view_root_control(&mut self, start: usize, control: &ControlCell) {
        if let Some(root) = self.cur.get_mut(start).filter(|r| r.control.is_none()) {
            root.control = Some(Rc::downgrade(control));
        }
    }

    /// Where the element named `name` was painted (content coordinates): this frame when it already
    /// was, else the last frame — what a `<Popover>` anchors to.
    pub(crate) fn bounds_of(&self, name: &str) -> Option<Rect> {
        self.cur.iter().rev().chain(self.prev.iter().rev()).find(|r| r.slot.name.as_deref() == Some(name)).map(|r| r.bounds)
    }

    /// Marks the element registered last as disabled (see `Registered::enabled`).
    pub(crate) fn disable_last(&mut self) {
        if let Some(last) = self.cur.last_mut() {
            last.enabled = false;
        }
    }

    /// `KeyPreview` (a view root's property): the root gets KeyDown/KeyPress/KeyUp before the
    /// focused element, and a handled key stops there (WinForms `Form.KeyPreview`).
    pub fn set_key_preview(&mut self, on: bool) {
        self.key_preview = on;
    }

    /// The first focusable element painted after the element `id` last frame (what a label's
    /// mnemonic moves the focus to: the control that follows it in the Tab order).
    pub(crate) fn focus_after(&self, id: &str) -> Option<FocusId> {
        let at = self.prev.iter().position(|r| r.slot.id == id)?;
        self.prev
            .iter()
            .skip(at + 1)
            .filter(|r| r.enabled && r.slot.focus_id.is_some())
            .find(|r| r.with_control(|c| c.can_select()).unwrap_or(false))
            .and_then(|r| r.slot.focus_id)
    }

    /// The element named `name` (`x:Name`) last frame.
    pub(crate) fn find_named(&self, name: &str) -> Option<(String, Option<FocusId>, bool)> {
        self.prev.iter().rev().find(|r| r.slot.name.as_deref() == Some(name)).map(|r| (r.slot.id.clone(), r.slot.focus_id, r.slot.keyboard_click))
    }

    /// Raises `event` on the element `id` of last frame (through its control) with `args`; `None`
    /// when no such element was painted or it is disabled.
    pub(crate) fn raise_on<A: EventArgs + Clone>(&mut self, d: &mut Dispatch<'_>, id: &str, event: &'static str, args: A) -> Option<A> {
        let target = self.find(id).filter(|r| r.enabled)?;
        Some(raise_to(d, &target, event, args))
    }

    /// Whether the element holding the focus is a button that clicks on Enter itself (the view's
    /// AcceptButton then leaves Enter to it).
    pub(crate) fn focused_clicks_on_enter(&self, focus: Option<FocusId>) -> bool {
        focus.and_then(|id| self.find_focus(id)).is_some_and(|r| r.slot.keyboard_click)
    }

    /// The id of the element under the pointer (last routed frame).
    pub(crate) fn hot_id(&self) -> Option<&str> {
        self.hot.as_deref()
    }

    /// The id of the element holding the mouse capture (a button pressed on it), and which button.
    pub(crate) fn captured(&self) -> Option<(&str, MouseButton)> {
        self.capture.as_ref().map(|c| (c.id.as_str(), c.button))
    }

    /// The registered view root (the sender of the view events), detached before its first paint.
    fn root_registered(&self) -> Option<Registered> {
        let root = self.root.clone()?;
        Some(self.find(&root.id).unwrap_or_else(|| Registered::detached(root)))
    }

    /// The message pre-filter (`Control::wnd_proc`) of `target`'s control; `true` when it ate the
    /// message.
    fn pre_filter(target: &Registered, mut msg: Message) -> bool {
        target.with_control(|c| c.wnd_proc(&mut msg)).unwrap_or(false)
    }

    /// WinForms' key pre-processing of a key-down on `target`: `wnd_proc`, then `process_cmd_key`
    /// on the target and each ancestor control, then — for a dialog key the target does not take
    /// as input (`is_input_key`) — `process_dialog_key` up the same chain. `true` when one of them
    /// consumed the key (no KeyDown/KeyPress follows).
    fn pre_process_key(&self, target: &Registered, keys: Keys) -> bool {
        if Self::pre_filter(target, Message::key(Message::WM_KEYDOWN, keys)) {
            return true;
        }
        let chain = self.chain(&target.slot.id);
        let mut msg = Message::key(Message::WM_KEYDOWN, keys);
        if chain.iter().any(|r| r.with_control(|c| c.process_cmd_key(&mut msg, keys)).unwrap_or(false)) {
            return true;
        }
        if !keys.is_dialog_key() || target.with_control(|c| c.is_input_key(keys)).unwrap_or(true) {
            return false;
        }
        chain.iter().any(|r| r.with_control(|c| c.process_dialog_key(keys)).unwrap_or(false))
    }

    fn find(&self, id: &str) -> Option<Registered> {
        self.prev.iter().rev().find(|r| r.slot.id == id).cloned()
    }

    fn find_focus(&self, focus: FocusId) -> Option<Registered> {
        self.prev.iter().rev().find(|r| r.slot.focus_id == Some(focus)).cloned()
    }

    /// The deepest element of the previous frame under the client point; `None` over a disabled one
    /// (a disabled control swallows the pointer without raising anything, like in WinForms).
    fn hit(&self, x: f32, y: f32) -> Option<Registered> {
        self.prev
            .iter()
            .rev()
            .find(|r| r.hit.right > r.hit.left && r.hit.bottom > r.hit.top && r.hit.contains(x, y))
            .filter(|r| r.enabled)
            .cloned()
    }

    /// `id` then each of its ancestors that was painted, innermost first.
    fn chain(&self, id: &str) -> Vec<Registered> {
        let mut out = Vec::new();
        let mut current = Some(id.to_string());
        while let Some(i) = current {
            if let Some(r) = self.find(&i) {
                out.push(r);
            }
            current = crate::design::parent_id_of(&i);
        }
        out
    }

    /// The first half of a frame: everything that precedes the controls' own reactions.
    /// `focus` has already run its own `begin_frame`; its recorded moves are read here.
    pub fn begin_frame(&mut self, input: &FrameInput<'_>, focus: &mut FocusRing, d: &mut Dispatch<'_>) -> BeginOutcome {
        let mut outcome = BeginOutcome::default();
        let frame = input.frame;
        self.post.clear();

        // View lifecycle: Load → Activated on the first frame; later, the window's focus edges.
        if let Some(root) = self.root_registered() {
            if !self.loaded {
                self.loaded = true;
                raise_to(d, &root, "OnLoad", EmptyEventArgs);
            }
            if self.window_active != Some(frame.window_focused) {
                let was = self.window_active.replace(frame.window_focused);
                if frame.window_focused {
                    raise_to(d, &root, "OnActivated", EmptyEventArgs);
                } else if was.is_some() {
                    raise_to(d, &root, "OnDeactivate", EmptyEventArgs);
                }
            }
            // What the window's chrome did since the last frame (the Kubuno title bar, a move or
            // resize, a DPI change): the view's own events of the same name.
            for event in self.window_events.drain(..) {
                use kubuno_desktop_controls::host::WindowEvent as W;
                match event {
                    W::TitleBarDoubleClick => {
                        raise_to(d, &root, "OnTitleBarDoubleClick", EmptyEventArgs);
                    }
                    W::HelpButtonClicked => {
                        raise_to(d, &root, "OnHelpButtonClicked", EmptyEventArgs);
                    }
                    W::ResizeBegin => {
                        raise_to(d, &root, "OnResizeBegin", EmptyEventArgs);
                    }
                    W::ResizeEnd => {
                        raise_to(d, &root, "OnResizeEnd", EmptyEventArgs);
                    }
                    W::DpiChanged { old, new } => {
                        raise_to(d, &root, "OnDpiChanged", crate::events::DpiChangedEventArgs { old_dpi: old, new_dpi: new });
                    }
                    W::CaptionButtonClick(id) => {
                        raise_to(d, &root, "OnCaptionButtonClick", crate::events::CaptionButtonEventArgs { id });
                    }
                    W::MdiChildActivate => {
                        raise_to(d, &root, "OnMdiChildActivate", EmptyEventArgs);
                    }
                }
            }
            // The window moved on screen (EVT-6): the view root's Move / LocationChanged, the
            // form's own in WinForms (its painted bounds, relative to the window, did not move).
            if self.window_origin.replace(frame.client_origin).is_some_and(|o| o != frame.client_origin) {
                raise_to(d, &root, "OnMove", EmptyEventArgs);
                raise_to(d, &root, "OnLocationChanged", EmptyEventArgs);
            }
        }

        // Focus.
        for change in focus.take_changes() {
            self.route_focus(change, focus, d);
        }

        // Pointer: enter/leave, move, hover.
        let (px, py) = frame.mouse;
        let outside = frame.pointer_outside();
        let hit = if outside { None } else { self.hit(px, py) };
        let hit_id = hit.as_ref().map(|r| r.slot.id.clone());
        if hit_id != self.hot {
            if let Some(old) = self.hot.take().and_then(|id| self.find(&id)) {
                raise_to(d, &old, "OnMouseLeave", EmptyEventArgs);
            }
            if let Some(new) = &hit {
                raise_to(d, new, "OnMouseEnter", EmptyEventArgs);
            }
            self.hot = hit_id;
            self.still_since = input.now_ms;
            self.hover_fired = false;
        }
        let target = match &self.capture {
            Some(c) => self.find(&c.id),
            None => hit.clone(),
        };
        let held = self.capture.as_ref().map(|c| c.button).unwrap_or_default();
        if self.last_mouse != Some((px, py)) && !outside {
            if let Some(t) = &target {
                if !Self::pre_filter(t, mouse_msg(Message::WM_MOUSEMOVE, t, frame, 0.0)) {
                    raise_to(d, t, "OnMouseMove", mouse_args(t, frame, held, 0, 0.0));
                }
            }
            self.still_since = input.now_ms;
            self.hover_fired = false;
        }
        self.last_mouse = Some((px, py));
        if let Some(hot) = self.hot.clone().and_then(|id| self.find(&id)) {
            if !self.hover_fired {
                let rested = input.now_ms.saturating_sub(self.still_since);
                if rested >= HOVER_DELAY_MS {
                    self.hover_fired = true;
                    raise_to(d, &hot, "OnMouseHover", EmptyEventArgs);
                } else {
                    outcome.repaint_after = Some((HOVER_DELAY_MS - rested) as u32);
                }
            }
        }

        // Buttons: presses (capture), releases (queued after the controls' own Click).
        let now = [frame.mouse_down, frame.right_down, frame.middle_down];
        for (i, button) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter().enumerate() {
            let (was, is) = (self.buttons[i], now[i]);
            if is && !was && self.capture.is_none() {
                if let Some(t) = hit.as_ref().filter(|t| !Self::pre_filter(t, mouse_msg(button_msg(button, true), t, frame, 0.0))) {
                    let clicks = if button == MouseButton::Left { frame.click_count.max(1) } else { 1 };
                    raise_to(d, t, "OnMouseDown", mouse_args(t, frame, button, clicks, 0.0));
                    self.capture = Some(Capture { id: t.slot.id.clone(), button, clicks });
                }
            }
            if !is && was && self.capture.as_ref().is_some_and(|c| c.button == button) {
                if let Some(c) = self.capture.take() {
                    self.queue_release(c, frame, outside);
                }
            }
        }
        self.buttons = now;

        // Wheel.
        if frame.wheel.1 != 0.0 {
            if let Some(t) = hit.as_ref().filter(|t| !Self::pre_filter(t, mouse_msg(Message::WM_MOUSEWHEEL, t, frame, frame.wheel.1))) {
                raise_to(d, t, "OnMouseWheel", mouse_args(t, frame, MouseButton::None, 0, frame.wheel.1));
            }
        }

        // Keys, to the focused element (the view root first with KeyPreview; the root alone when
        // nothing has the focus and it previews them).
        match focus.focused().and_then(|id| self.find_focus(id)) {
            Some(target) => self.route_keys(&target, input.events, d, &mut outcome.consumed),
            None if self.key_preview => {
                if let Some(root) = self.root_registered() {
                    self.route_keys(&root, input.events, d, &mut outcome.consumed);
                }
            }
            None => {}
        }
        outcome
    }

    /// KeyPreview: raises `event` on the view root before the focused `target`; `true` when a
    /// handler marked it handled (the target does not get it).
    fn preview<A: EventArgs + Clone + Handled>(&self, target: &Registered, d: &mut Dispatch<'_>, event: &'static str, args: A) -> bool {
        if !self.key_preview {
            return false;
        }
        let Some(root) = self.root_registered().filter(|r| r.slot.id != target.slot.id) else { return false };
        raise_to(d, &root, event, args).handled()
    }

    fn queue_release(&mut self, c: Capture, frame: &Frame, outside: bool) {
        let Some(t) = self.find(&c.id) else { return };
        if Self::pre_filter(&t, mouse_msg(button_msg(c.button, false), &t, frame, 0.0)) {
            return;
        }
        let inside = !outside && t.hit.contains(frame.mouse.0, frame.mouse.1);
        if inside {
            if c.clicks >= 2 && c.button == MouseButton::Left && t.slot.standard_double_click {
                self.post.push((t.clone(), Post::Mouse("OnDoubleClick", mouse_args(&t, frame, c.button, c.clicks, 0.0))));
                self.post.push((t.clone(), Post::Mouse("OnMouseDoubleClick", mouse_args(&t, frame, c.button, c.clicks, 0.0))));
            } else {
                if c.button == MouseButton::Left && !t.slot.native_click {
                    self.post.push((t.clone(), Post::Mouse("OnClick", mouse_args(&t, frame, c.button, c.clicks, 0.0))));
                }
                self.post.push((t.clone(), Post::Mouse("OnMouseClick", mouse_args(&t, frame, c.button, c.clicks, 0.0))));
            }
        }
        self.post.push((t.clone(), Post::Mouse("OnMouseUp", mouse_args(&t, frame, c.button, c.clicks, 0.0))));
    }

    fn route_keys(&mut self, target: &Registered, events: &[InputEvent], d: &mut Dispatch<'_>, consumed: &mut Vec<usize>) {
        let slot = &target.slot;
        let mut suppress_press = false;
        let mut pending_click = false;
        for (i, e) in events.iter().enumerate() {
            match e {
                InputEvent::Key { vk: k, down: true, repeat, mods } => {
                    if pending_click {
                        pending_click = false;
                        raise_to(d, target, "OnClick", keyboard_click_args());
                    }
                    let keys = Keys::new(Key(*k), *mods);
                    if self.pre_process_key(target, keys) {
                        consumed.push(i);
                        suppress_press = true;
                        continue;
                    }
                    if self.preview(target, d, "OnKeyDown", KeyEventArgs::new(Key(*k), *mods)) {
                        consumed.push(i);
                        suppress_press = true;
                        continue;
                    }
                    let args = raise_to(d, target, "OnKeyDown", KeyEventArgs::new(Key(*k), *mods));
                    if args.handled {
                        consumed.push(i);
                    }
                    suppress_press = args.suppress_key_press;
                    if slot.keyboard_click && !args.handled && !*repeat {
                        if *k == vk::ENTER {
                            raise_to(d, target, "OnClick", keyboard_click_args());
                        } else if *k == vk::SPACE {
                            pending_click = true;
                        }
                    }
                }
                InputEvent::Text(text) => {
                    if suppress_press {
                        consumed.push(i);
                    } else {
                        let mut handled = false;
                        for c in text.chars() {
                            if Self::pre_filter(target, Message::char(c)) {
                                handled = true;
                                continue;
                            }
                            if !target.with_control(|ctl| ctl.is_input_char(c)).unwrap_or(true) {
                                continue;
                            }
                            if self.preview(target, d, "OnKeyPress", KeyPressEventArgs::new(c)) {
                                handled = true;
                                continue;
                            }
                            handled |= raise_to(d, target, "OnKeyPress", KeyPressEventArgs::new(c)).handled;
                        }
                        if handled {
                            consumed.push(i);
                        }
                    }
                    suppress_press = false;
                    if pending_click {
                        pending_click = false;
                        raise_to(d, target, "OnClick", keyboard_click_args());
                    }
                }
                InputEvent::Key { vk: k, down: false, mods, .. } => {
                    if pending_click {
                        pending_click = false;
                        raise_to(d, target, "OnClick", keyboard_click_args());
                    }
                    if Self::pre_filter(target, Message::key(Message::WM_KEYUP, Keys::new(Key(*k), *mods))) {
                        consumed.push(i);
                        continue;
                    }
                    if self.preview(target, d, "OnKeyUp", KeyEventArgs::new(Key(*k), *mods)) {
                        consumed.push(i);
                        continue;
                    }
                    if raise_to(d, target, "OnKeyUp", KeyEventArgs::new(Key(*k), *mods)).handled {
                        consumed.push(i);
                    }
                }
                _ => {}
            }
        }
        if pending_click {
            raise_to(d, target, "OnClick", keyboard_click_args());
        }
    }

    /// One focus move → the WinForms sequence (module doc), with validation.
    fn route_focus(&mut self, change: FocusChange, focus: &mut FocusRing, d: &mut Dispatch<'_>) {
        let old = change.from.and_then(|id| self.find_focus(id));
        let new = change.to.and_then(|id| self.find_focus(id));
        let old_chain = old.as_ref().map(|r| self.chain(&r.slot.id)).unwrap_or_default();
        let new_chain = new.as_ref().map(|r| self.chain(&r.slot.id)).unwrap_or_default();
        let in_chain = |chain: &[Registered], r: &Registered| chain.iter().any(|c| c.slot.id == r.slot.id);
        // Innermost first.
        let leaving: Vec<Registered> = old_chain.iter().filter(|r| !in_chain(&new_chain, r)).cloned().collect();
        // Outermost first.
        let entering: Vec<Registered> = new_chain.iter().rev().filter(|r| !in_chain(&old_chain, r)).cloned().collect();

        for r in &entering {
            raise_to(d, r, "OnEnter", EmptyEventArgs);
        }
        if let Some(n) = &new {
            raise_to(d, n, "OnGotFocus", EmptyEventArgs);
        }
        let Some(o) = old else {
            return;
        };
        let pointer = change.cause == FocusCause::Pointer;
        if pointer || change.cause == FocusCause::Removed {
            raise_to(d, &o, "OnLostFocus", EmptyEventArgs);
        }
        for r in &leaving {
            raise_to(d, r, "OnLeave", EmptyEventArgs);
        }
        if change.cause == FocusCause::Removed {
            return;
        }
        // `CausesValidation = false` on the element receiving the focus (a Cancel button): the one
        // losing it is not validated (WinForms).
        if new.as_ref().is_some_and(|n| !n.slot.causes_validation) {
            if !pointer {
                raise_to(d, &o, "OnLostFocus", EmptyEventArgs);
            }
            return;
        }
        let validating = raise_to(d, &o, "OnValidating", CancelEventArgs::default());
        if validating.cancel {
            // The focus stays where it was: undo the move, balance the new side's events.
            focus.restore(change.from);
            if let Some(n) = &new {
                raise_to(d, n, "OnLostFocus", EmptyEventArgs);
            }
            for r in entering.iter().rev() {
                raise_to(d, r, "OnLeave", EmptyEventArgs);
            }
            if pointer {
                for r in leaving.iter().rev() {
                    raise_to(d, r, "OnEnter", EmptyEventArgs);
                }
                raise_to(d, &o, "OnGotFocus", EmptyEventArgs);
            }
            return;
        }
        raise_to(d, &o, "OnValidated", EmptyEventArgs);
        if !pointer {
            raise_to(d, &o, "OnLostFocus", EmptyEventArgs);
        }
    }

    /// The second half of a frame, after the paint: the queued MouseClick/MouseUp (after
    /// a control's own Click), the layout events of every element whose bounds changed,
    /// Shown after the view's first frame. Then this frame's elements become the geometry
    /// the next frame routes against.
    pub fn end_frame(&mut self, d: &mut Dispatch<'_>) {
        for (t, post) in std::mem::take(&mut self.post) {
            match post {
                Post::Mouse(event, args) => {
                    raise_to(d, &t, event, args);
                }
            }
        }

        let mut bounds = HashMap::with_capacity(self.cur.len());
        for r in &self.cur {
            if let Some(old) = self.last_bounds.get(&r.slot.id) {
                let b = r.bounds;
                if (old.right - old.left, old.bottom - old.top) != (b.right - b.left, b.bottom - b.top) {
                    raise_to(d, r, "OnResize", EmptyEventArgs);
                    raise_to(d, r, "OnSizeChanged", EmptyEventArgs);
                }
                if (old.left, old.top) != (b.left, b.top) {
                    raise_to(d, r, "OnMove", EmptyEventArgs);
                    raise_to(d, r, "OnLocationChanged", EmptyEventArgs);
                }
            }
            bounds.insert(r.slot.id.clone(), r.bounds);
        }
        self.last_bounds = bounds;

        if self.loaded && !self.shown && !self.hidden {
            if let Some(root) = self.root.clone() {
                self.shown = true;
                let reg = self.cur.iter().find(|r| r.slot.id == root.id).cloned().unwrap_or_else(|| Registered::detached(root));
                raise_to(d, &reg, "OnShown", EmptyEventArgs);
            }
        }

        self.prev = std::mem::take(&mut self.cur);
    }

    /// The view root and its bounds at the last frame: the sender of the view events.
    pub fn root_sender(&self) -> Option<(Rc<SlotEvents>, Rect)> {
        let root = self.root.clone()?;
        let bounds = self.find(&root.id).map(|r| r.bounds).unwrap_or_default();
        Some((root, bounds))
    }

    /// Raises FormClosing on the view root (EVT-6) and returns its args once every handler
    /// ran: `cancel` set keeps the window open.
    pub fn form_closing(&mut self, d: &mut Dispatch<'_>, reason: CloseReason) -> FormClosingEventArgs {
        let args = FormClosingEventArgs { reason, cancel: false };
        match self.root_registered() {
            Some(root) => raise_to(d, &root, "OnFormClosing", args),
            None => args,
        }
    }

    /// The end of the view's life, after a FormClosing nobody cancelled: FormClosed, then
    /// Deactivate when the window was active (`EVENTS.md` §3's closing order).
    pub fn form_closed(&mut self, d: &mut Dispatch<'_>, reason: CloseReason) {
        let Some(root) = self.root_registered() else { return };
        raise_to(d, &root, "OnFormClosed", FormClosedEventArgs { reason });
        if self.window_active == Some(true) {
            self.window_active = Some(false);
            raise_to(d, &root, "OnDeactivate", EmptyEventArgs);
        }
    }
}

/// The args of a Click raised from the keyboard (Space/Enter on a button): WinForms passes
/// `EventArgs.Empty`; here Click always carries `MouseEventArgs` so a `&MouseEventArgs` handler
/// runs for both, with no button and no click (`button == MouseButton::None`, `clicks == 0`).
fn keyboard_click_args() -> MouseEventArgs {
    MouseEventArgs::default()
}

/// The message [`InputRouter::pre_filter`] shows a control for a pointer event over it.
fn mouse_msg(msg: u32, t: &Registered, frame: &Frame, wheel_notches: f32) -> Message {
    Message::mouse(msg, frame.mouse.0 - t.client.left, frame.mouse.1 - t.client.top, wheel_notches)
}

/// `WM_xBUTTONDOWN` / `WM_xBUTTONUP` for `button`.
fn button_msg(button: MouseButton, down: bool) -> u32 {
    match (button, down) {
        (MouseButton::Right, true) => Message::WM_RBUTTONDOWN,
        (MouseButton::Right, false) => Message::WM_RBUTTONUP,
        (MouseButton::Middle, true) => Message::WM_MBUTTONDOWN,
        (MouseButton::Middle, false) => Message::WM_MBUTTONUP,
        (_, true) => Message::WM_LBUTTONDOWN,
        (_, false) => Message::WM_LBUTTONUP,
    }
}

fn mouse_args(t: &Registered, frame: &Frame, button: MouseButton, clicks: u8, delta: f32) -> MouseEventArgs {
    MouseEventArgs {
        button,
        clicks,
        x: frame.mouse.0 - t.client.left,
        y: frame.mouse.1 - t.client.top,
        delta,
        mods: frame.mods,
    }
}

#[cfg(test)]
mod tests {
    //! Scripted frame sequences asserting the exact WinForms orders of `EVENTS.md` §3.
    //! Each test element names a handler for every event it may raise (`"<id>:<Event>"`),
    //! so the frame's returned [`ViewEvent`]s are the log; a "paint" is a `register` call
    //! per element, like `DesignSlot::paint` does.

    use super::*;
    use crate::binding::{MapViewModel, Value};
    use kubuno_desktop_controls::host::Modifiers;

    const ALL: &[&str] = &[
        "OnClick",
        "OnDoubleClick",
        "OnMouseClick",
        "OnMouseDoubleClick",
        "OnMouseDown",
        "OnMouseUp",
        "OnMouseMove",
        "OnMouseEnter",
        "OnMouseLeave",
        "OnMouseHover",
        "OnMouseWheel",
        "OnKeyDown",
        "OnKeyPress",
        "OnKeyUp",
        "OnEnter",
        "OnGotFocus",
        "OnLeave",
        "OnLostFocus",
        "OnValidating",
        "OnValidated",
        "OnResize",
        "OnMove",
        "OnSizeChanged",
        "OnLocationChanged",
        "OnLoad",
        "OnShown",
        "OnActivated",
        "OnDeactivate",
        "OnFormClosing",
        "OnFormClosed",
    ];

    fn slot(id: &str, element: &'static str, name: Option<&str>) -> Rc<SlotEvents> {
        let mut s = SlotEvents::new(id, element);
        s.name = name.map(str::to_string);
        s.focus_id = name.map(FocusId::of);
        for e in ALL {
            let label = if id.is_empty() { "root".to_string() } else { name.unwrap_or(id).to_string() };
            s = s.with_handler(e, format!("{label}:{}", &e[2..]));
        }
        Rc::new(s)
    }

    fn button(id: &str, name: &str) -> Rc<SlotEvents> {
        let mut s = (*slot(id, "Button", Some(name))).clone();
        s.native_click = true;
        s.keyboard_click = true;
        s.standard_double_click = false;
        Rc::new(s)
    }

    fn frame(mouse: (f32, f32), down: bool, clicks: u8) -> Frame {
        Frame {
            size: (400.0, 300.0),
            mouse,
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 400.0, 300.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: clicks,
            window_focused: true,
        }
    }

    /// A view: a root panel with two children, painted every frame.
    struct Harness {
        router: InputRouter,
        focus: FocusRing,
        vm: MapViewModel,
        handlers: HandlerTable,
        elements: Vec<(Rc<SlotEvents>, Rect)>,
        now: u64,
        log: Vec<String>,
    }

    impl Harness {
        fn new(elements: Vec<(Rc<SlotEvents>, Rect)>) -> Self {
            let mut router = InputRouter::new();
            router.set_root(elements[0].0.clone());
            Self { router, focus: FocusRing::new(), vm: MapViewModel::new(), handlers: HandlerTable::new(), elements, now: 0, log: Vec::new() }
        }

        /// One frame; `during_paint` runs between the halves (a control's own reaction).
        fn run(&mut self, f: Frame, keys: &[InputEvent], during_paint: impl FnOnce(&mut Dispatch<'_>)) -> BeginOutcome {
            let mut events = Vec::new();
            self.focus.begin_frame(&f);
            let outcome = {
                let mut d = Dispatch { vm: &mut self.vm, handlers: &mut self.handlers, events: &mut events };
                let input = FrameInput { frame: &f, now_ms: self.now, events: keys };
                let outcome = self.router.begin_frame(&input, &mut self.focus, &mut d);
                for (s, b) in &self.elements {
                    self.router.register(s.clone(), *b);
                    if let Some(id) = s.focus_id {
                        self.focus.register(id, *b);
                    }
                }
                during_paint(&mut d);
                self.router.end_frame(&mut d);
                outcome
            };
            self.focus.end_frame();
            for e in events {
                if let (Some(h), ViewEventKind::Other { .. }) = (&e.handler, &e.kind) {
                    self.log.push(h.clone());
                } else if let Some(h) = e.handler {
                    self.log.push(format!("{h} (native)"));
                }
            }
            outcome
        }

        fn frame(&mut self, mouse: (f32, f32), down: bool, clicks: u8) {
            self.run(frame(mouse, down, clicks), &[], |_| {});
        }

        fn take(&mut self) -> Vec<String> {
            std::mem::take(&mut self.log)
        }

        /// Two frames with the pointer at `pos` (the first one has no geometry to route
        /// against yet), then forgets what they raised.
        fn settle(&mut self, pos: (f32, f32)) {
            self.frame(pos, false, 0);
            self.frame(pos, false, 0);
            self.take();
        }
    }

    fn view() -> Harness {
        Harness::new(vec![
            (slot("", "Panel", None), Rect::new(0.0, 0.0, 400.0, 300.0)),
            (slot("0", "Label", Some("label")), Rect::new(10.0, 10.0, 110.0, 40.0)),
            (slot("1", "Label", Some("other")), Rect::new(10.0, 60.0, 110.0, 90.0)),
        ])
    }

    fn only(log: Vec<String>, prefix: &str) -> Vec<String> {
        log.into_iter().filter(|l| l.starts_with(prefix)).collect()
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn load_activated_then_shown_on_the_first_frame_only() {
        let mut h = view();
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(h.take(), strings(&["root:Load", "root:Activated", "root:Shown"]));
        h.frame((-1e4, -1e4), false, 0);
        assert!(h.take().is_empty());
        let mut f = frame((-1e4, -1e4), false, 0);
        f.window_focused = false;
        h.run(f, &[], |_| {});
        f.window_focused = true;
        h.run(f, &[], |_| {});
        assert_eq!(h.take(), strings(&["root:Deactivate", "root:Activated"]));
    }

    #[test]
    // A press on a focusable (named) element focuses it first: Enter → GotFocus precede
    // MouseDown, as in WinForms.
    fn click_is_mouse_down_click_mouse_click_mouse_up() {
        let mut h = view();
        h.frame((-1e4, -1e4), false, 0);
        h.settle((20.0, 20.0));
        h.frame((20.0, 20.0), true, 1);
        h.frame((20.0, 20.0), false, 1);
        assert_eq!(only(h.take(), "label"), strings(&["label:Enter", "label:GotFocus", "label:MouseDown", "label:Click", "label:MouseClick", "label:MouseUp"]));
    }

    #[test]
    fn a_native_click_sits_between_mouse_down_and_mouse_click() {
        let mut h = Harness::new(vec![
            (slot("", "Panel", None), Rect::new(0.0, 0.0, 400.0, 300.0)),
            (button("0", "ok"), Rect::new(10.0, 10.0, 110.0, 40.0)),
        ]);
        h.settle((20.0, 20.0));
        h.frame((20.0, 20.0), true, 1);
        // The release frame: the button's own node raises Click while painting.
        h.run(frame((20.0, 20.0), false, 1), &[], |d| {
            d.events.push(ViewEvent { focus_id: None, handler: Some("ok:Click".into()), kind: ViewEventKind::Clicked });
        });
        assert_eq!(only(h.take(), "ok"), strings(&["ok:Enter", "ok:GotFocus", "ok:MouseDown", "ok:Click (native)", "ok:MouseClick", "ok:MouseUp"]));
    }

    #[test]
    fn release_outside_after_a_press_inside_is_only_mouse_up() {
        let mut h = view();
        h.settle((20.0, 20.0));
        h.frame((20.0, 20.0), true, 1);
        h.frame((300.0, 200.0), false, 1);
        // Captured: the move outside still goes to the pressed label.
        assert_eq!(only(h.take(), "label"), strings(&["label:Enter", "label:GotFocus", "label:MouseDown", "label:MouseLeave", "label:MouseMove", "label:MouseUp"]));
    }

    #[test]
    fn double_click_sequence() {
        let mut h = view();
        h.settle((20.0, 20.0));
        h.frame((20.0, 20.0), true, 1);
        h.frame((20.0, 20.0), false, 1);
        h.frame((20.0, 20.0), true, 2);
        h.frame((20.0, 20.0), false, 2);
        assert_eq!(
            only(h.take(), "label"),
            strings(&[
                "label:Enter",
                "label:GotFocus",
                "label:MouseDown",
                "label:Click",
                "label:MouseClick",
                "label:MouseUp",
                "label:MouseDown",
                "label:DoubleClick",
                "label:MouseDoubleClick",
                "label:MouseUp",
            ])
        );
    }

    #[test]
    fn a_button_double_click_is_a_second_click() {
        let mut h = Harness::new(vec![
            (slot("", "Panel", None), Rect::new(0.0, 0.0, 400.0, 300.0)),
            (button("0", "ok"), Rect::new(10.0, 10.0, 110.0, 40.0)),
        ]);
        h.settle((20.0, 20.0));
        for clicks in [1, 2] {
            h.frame((20.0, 20.0), true, clicks);
            h.frame((20.0, 20.0), false, clicks);
        }
        let log = only(h.take(), "ok");
        assert_eq!(log, strings(&["ok:Enter", "ok:GotFocus", "ok:MouseDown", "ok:MouseClick", "ok:MouseUp", "ok:MouseDown", "ok:MouseClick", "ok:MouseUp"]));
    }

    #[test]
    fn hover_sequence_enter_move_hover_once_leave() {
        let mut h = view();
        h.frame((300.0, 200.0), false, 0);
        h.take();
        h.now = 1000;
        h.frame((20.0, 20.0), false, 0);
        h.now = 1100;
        h.frame((22.0, 21.0), false, 0);
        h.now = 1300;
        let pending = h.run(frame((22.0, 21.0), false, 0), &[], |_| {});
        assert_eq!(pending.repaint_after, Some(200), "asks for the frame that raises MouseHover");
        h.now = 1500;
        h.frame((22.0, 21.0), false, 0);
        h.now = 3000;
        h.frame((22.0, 21.0), false, 0);
        h.frame((300.0, 200.0), false, 0);
        assert_eq!(
            only(h.take(), "label"),
            strings(&["label:MouseEnter", "label:MouseMove", "label:MouseMove", "label:MouseHover", "label:MouseLeave"])
        );
    }

    #[test]
    fn the_deepest_element_gets_the_mouse_and_its_parent_gets_leave() {
        let mut h = view();
        h.settle((300.0, 200.0));
        h.frame((20.0, 20.0), false, 0);
        let log = h.take();
        assert_eq!(log, strings(&["root:MouseLeave", "label:MouseEnter", "label:MouseMove"]));
    }

    #[test]
    fn wheel_goes_to_the_element_under_the_pointer() {
        let mut h = view();
        h.settle((20.0, 20.0));
        let mut f = frame((20.0, 20.0), false, 0);
        f.wheel = (0.0, 1.0);
        h.run(f, &[], |_| {});
        assert_eq!(only(h.take(), "label"), strings(&["label:MouseWheel"]));
    }

    fn key(k: u16, down: bool) -> InputEvent {
        InputEvent::Key { vk: k, down, repeat: false, mods: Modifiers::NONE }
    }

    #[test]
    fn keys_go_to_the_focused_element_in_order() {
        let mut h = view();
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("label");
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        let keys = [key(vk::letter('A'), true), InputEvent::Text("a".into()), key(vk::letter('A'), false)];
        h.run(frame((-1e4, -1e4), false, 0), &keys, |_| {});
        assert_eq!(h.take(), strings(&["label:KeyDown", "label:KeyPress", "label:KeyUp"]));
    }

    #[test]
    fn a_handled_key_down_is_consumed_and_can_suppress_the_key_press() {
        let mut h = view();
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("label");
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        h.handlers.insert_typed(
            "label:KeyDown",
            Box::new(|_vm, _sender, args| {
                if let Some(k) = args.downcast_mut::<KeyEventArgs>() {
                    k.suppress();
                }
            }),
        );
        let keys = [key(vk::letter('A'), true), InputEvent::Text("a".into()), key(vk::letter('A'), false)];
        let outcome = h.run(frame((-1e4, -1e4), false, 0), &keys, |_| {});
        assert_eq!(outcome.consumed, vec![0, 1]);
        assert_eq!(h.take(), strings(&["label:KeyDown", "label:KeyUp"]));
    }

    #[test]
    fn space_or_enter_on_a_button_raises_click_alone() {
        let mut h = Harness::new(vec![
            (slot("", "Panel", None), Rect::new(0.0, 0.0, 400.0, 300.0)),
            (button("0", "ok"), Rect::new(10.0, 10.0, 110.0, 40.0)),
        ]);
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("ok");
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        let keys = [key(vk::SPACE, true), InputEvent::Text(" ".into()), key(vk::SPACE, false), key(vk::ENTER, true), key(vk::ENTER, false)];
        h.run(frame((-1e4, -1e4), false, 0), &keys, |_| {});
        assert_eq!(
            h.take(),
            strings(&["ok:KeyDown", "ok:KeyPress", "ok:Click", "ok:KeyUp", "ok:KeyDown", "ok:Click", "ok:KeyUp"])
        );
    }

    /// Two focusable fields inside a named container, the second one outside it.
    fn form() -> Harness {
        Harness::new(vec![
            (slot("", "Panel", None), Rect::new(0.0, 0.0, 400.0, 300.0)),
            (slot("0", "GroupBox", None), Rect::new(0.0, 0.0, 200.0, 100.0)),
            (slot("0.0", "TextField", Some("first")), Rect::new(10.0, 10.0, 110.0, 40.0)),
            (slot("1", "TextField", Some("second")), Rect::new(10.0, 160.0, 110.0, 190.0)),
        ])
    }

    fn focus_log(log: Vec<String>) -> Vec<String> {
        let focus = ["Enter", "Leave", "GotFocus", "LostFocus", "Validating", "Validated"];
        log.into_iter().filter(|l| focus.iter().any(|f| l.ends_with(&format!(":{f}")))).collect()
    }

    #[test]
    fn focus_by_keyboard_sequence() {
        let mut h = form();
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("first");
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(focus_log(h.take()), strings(&["root:Enter", "0:Enter", "first:Enter", "first:GotFocus"]));
        h.focus.step(true); // Tab
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(
            focus_log(h.take()),
            strings(&["second:Enter", "second:GotFocus", "first:Leave", "0:Leave", "first:Validating", "first:Validated", "first:LostFocus"])
        );
    }

    #[test]
    fn focus_by_mouse_sequence() {
        let mut h = form();
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("first");
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        h.frame((20.0, 170.0), false, 0);
        h.frame((20.0, 170.0), true, 1);
        assert_eq!(
            focus_log(h.take()),
            strings(&["second:Enter", "second:GotFocus", "first:LostFocus", "first:Leave", "0:Leave", "first:Validating", "first:Validated"])
        );
        assert!(h.focus.is_focused("second"));
    }

    #[test]
    fn focus_events_come_before_mouse_down() {
        let mut h = form();
        h.settle((20.0, 170.0));
        h.frame((20.0, 170.0), true, 1);
        let log: Vec<String> = h.take().into_iter().filter(|l| l.starts_with("second")).collect();
        assert_eq!(log, strings(&["second:Enter", "second:GotFocus", "second:MouseDown"]));
    }

    #[test]
    fn a_cancelled_validating_keeps_the_focus() {
        let mut h = form();
        h.frame((-1e4, -1e4), false, 0);
        h.focus.focus("first");
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        h.handlers.insert_typed(
            "first:Validating",
            Box::new(|vm, _sender, args| {
                if let Some(c) = args.downcast_mut::<CancelEventArgs>() {
                    c.cancel = true;
                }
                vm.set("Error", Value::Str("required".into()));
            }),
        );
        h.focus.step(true);
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(
            focus_log(h.take()),
            strings(&["second:Enter", "second:GotFocus", "first:Leave", "0:Leave", "first:Validating", "second:LostFocus", "second:Leave"])
        );
        assert!(h.focus.is_focused("first"), "the focus stays on the element that failed validation");
        assert_eq!(h.vm.get("Error"), Some(Value::Str("required".into())));
    }

    #[test]
    fn resize_and_move_follow_bounds_changes() {
        let mut h = view();
        h.frame((-1e4, -1e4), false, 0);
        h.take();
        h.elements[1].1 = Rect::new(10.0, 10.0, 150.0, 40.0);
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(h.take(), strings(&["label:Resize", "label:SizeChanged"]));
        h.elements[1].1 = Rect::new(20.0, 10.0, 160.0, 40.0);
        h.frame((-1e4, -1e4), false, 0);
        assert_eq!(h.take(), strings(&["label:Move", "label:LocationChanged"]));
    }

    #[test]
    fn slot_events_read_handlers_and_aliases_from_the_xml() {
        let p = crate::syntax::parse(r#"<Panel OnLoad="loaded"><Switch x:Name="dark" OnToggled="dark_toggled" OnMouseDown="pressed"/></Panel>"#);
        let doc = <crate::ast::Document as crate::ast::AstNode>::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        let panel = SlotEvents::from_element(&root, crate::registry::lookup("Panel").unwrap(), true);
        assert_eq!(panel.handler("OnLoad"), Some("loaded"));
        let sw = root.children().next().unwrap();
        let switch = SlotEvents::from_element(&sw, crate::registry::lookup("Switch").unwrap(), false);
        assert_eq!(switch.handler("OnCheckedChanged"), Some("dark_toggled"));
        assert_eq!(switch.handler("OnMouseDown"), Some("pressed"));
        assert_eq!(switch.focus_id, Some(FocusId::of("dark")));
        assert!(!switch.native_click && switch.standard_double_click);
        let b = crate::syntax::parse(r#"<Button OnLoad="x"/>"#);
        let doc = <crate::ast::Document as crate::ast::AstNode>::cast(b.syntax()).unwrap();
        let button = SlotEvents::from_element(&doc.root_element().unwrap(), crate::registry::lookup("Button").unwrap(), false);
        assert_eq!(button.handler("OnLoad"), None, "view events are read on the root only");
        assert!(button.native_click && button.keyboard_click && !button.standard_double_click);
    }

    #[test]
    fn legacy_handlers_get_the_legacy_value() {
        let mut h = view();
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = seen.clone();
        h.handlers.insert("label:MouseDown", Box::new(move |_vm, v| log.borrow_mut().push(v)));
        h.frame((20.0, 20.0), false, 0);
        h.frame((20.0, 20.0), true, 1);
        assert_eq!(*seen.borrow(), vec![Value::Bool(true)]);
    }

    #[test]
    fn a_window_move_raises_the_roots_move_and_location_changed() {
        let mut h = view();
        h.settle((-1e4, -1e4));
        let mut f = frame((-1e4, -1e4), false, 0);
        f.client_origin = (120.0, 80.0);
        h.run(f, &[], |_| {});
        assert_eq!(h.take(), strings(&["root:Move", "root:LocationChanged"]));
        h.run(f, &[], |_| {});
        assert!(h.take().is_empty(), "no move, no event");
    }

    /// Runs FormClosing (and, when nobody cancelled it, FormClosed) like the runtime does,
    /// returning whether the view closed.
    fn close(h: &mut Harness, reason: CloseReason) -> bool {
        let mut events = Vec::new();
        let mut d = Dispatch { vm: &mut h.vm, handlers: &mut h.handlers, events: &mut events };
        let args = h.router.form_closing(&mut d, reason);
        if !args.cancel {
            h.router.form_closed(&mut d, reason);
        }
        h.log.extend(events.into_iter().filter_map(|e| e.handler));
        !args.cancel
    }

    #[test]
    fn closing_raises_form_closing_then_form_closed_then_deactivate() {
        let mut h = view();
        h.settle((-1e4, -1e4));
        let reasons = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = reasons.clone();
        h.handlers.insert_typed(
            "root:FormClosed",
            Box::new(move |_vm, sender, args| {
                assert_eq!(sender.element, "Panel");
                seen.borrow_mut().push(args.downcast_ref::<FormClosedEventArgs>().map(|a| a.reason));
            }),
        );
        assert!(close(&mut h, CloseReason::UserClosing));
        assert_eq!(h.take(), strings(&["root:FormClosing", "root:FormClosed", "root:Deactivate"]));
        assert_eq!(*reasons.borrow(), [Some(CloseReason::UserClosing)]);
    }

    #[test]
    fn a_cancelled_form_closing_keeps_the_view_and_raises_nothing_else() {
        let mut h = view();
        h.settle((-1e4, -1e4));
        let ask = Rc::new(std::cell::Cell::new(true));
        let unsaved = ask.clone();
        h.handlers.insert_typed(
            "root:FormClosing",
            Box::new(move |_vm, _sender, args| {
                let reason = args.downcast_ref::<FormClosingEventArgs>().map(|a| a.reason);
                assert_eq!(reason, Some(CloseReason::ApplicationExitCall));
                if let Some(c) = args.as_cancelable_mut() {
                    c.set_cancel(unsaved.get());
                }
            }),
        );
        assert!(!close(&mut h, CloseReason::ApplicationExitCall));
        assert_eq!(h.take(), strings(&["root:FormClosing"]));
        // Still alive: input keeps routing.
        h.frame((20.0, 20.0), false, 0);
        assert!(h.take().contains(&"label:MouseEnter".to_string()));
        // The user saved: the next close goes through.
        ask.set(false);
        assert!(close(&mut h, CloseReason::ApplicationExitCall));
        assert_eq!(h.take(), strings(&["root:FormClosing", "root:FormClosed", "root:Deactivate"]));
    }

    #[test]
    fn closing_an_inactive_window_does_not_raise_deactivate() {
        let mut h = view();
        let mut f = frame((-1e4, -1e4), false, 0);
        f.window_focused = false;
        h.run(f, &[], |_| {});
        h.take();
        assert!(close(&mut h, CloseReason::WindowsShutDown));
        assert_eq!(h.take(), strings(&["root:FormClosing", "root:FormClosed"]));
    }
}
