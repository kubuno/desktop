//! What an overridable `on_…` method receives: [`EventCx`] (the args, plus where the event goes
//! when the method's base behaviour raises it) and [`PaintEventCx`] (the canvas of `on_paint`).
//!
//! WinForms' `OnClick(EventArgs e)` raises `Click` by calling the delegates stored on the
//! control. A Kubuno control raises to two audiences: the Rust subscribers stored on the control
//! ([`super::EventMap`], `button.click().subscribe(…)`) and, for an element of a `.kbview`, the
//! handler its `OnClick="…"` attribute names, which needs the frame's view model. That second
//! audience is the [`RaiseSink`] an `EventCx` carries: the caller that invokes `on_click` (the
//! input router, a node, [`super::ControlHost`]) lends it for the duration of the call, so an
//! override stays a one-argument method — `fn on_click(&mut self, e: &mut EventCx<'_,
//! MouseEventArgs>)` — and `self.base_mut().on_click(e)` raises exactly like WinForms'
//! `base.OnClick(e)`.

use std::ops::{Deref, DerefMut};

use kubuno_controls::ControlCanvas;
use kubuno_ui::graphics::Graphics;
use kubuno_ui::{Rect, WidgetState};

use super::Component;
use crate::events::{ElementRef, EventArgs, PaintEventArgs};

/// Where an event raised by a control's base behaviour goes besides its Rust subscribers: the
/// `.kbview` handler of the element (the router's and the nodes' sink), or a host's own log.
pub trait RaiseSink {
    /// The sender of the events raised through this sink (the element, its bounds).
    fn sender(&self) -> ElementRef<'_>;
    /// Raises `event` (its attribute name, `"OnClick"`) to this sink's audience.
    fn raise(&mut self, event: &'static str, args: &mut dyn EventArgs);
}

/// The argument of an overridable `on_…` method: the event args (reached through `Deref`, so
/// `e.x`, `e.handled = true` work as on the args themselves) and the sink the base behaviour
/// raises to.
///
/// ```
/// use kubuno_views::component::EventCx;
/// use kubuno_views::events::{MouseButton, MouseEventArgs};
///
/// let mut args = MouseEventArgs { button: MouseButton::Left, clicks: 1, ..Default::default() };
/// let e = EventCx::new(&mut args);
/// assert_eq!(e.clicks, 1);
/// assert!(!e.has_sink() && e.event_name().is_none());
/// ```
pub struct EventCx<'a, A: ?Sized = dyn EventArgs> {
    args: &'a mut A,
    sink: Option<&'a mut dyn RaiseSink>,
    event: Option<&'static str>,
    origin: Option<&'static str>,
}

impl<'a, A: ?Sized + EventArgs + AsDynArgs> EventCx<'a, A> {
    /// Args with no sink: the base behaviour raises to the control's Rust subscribers only (a
    /// call from Rust code, `self.on_click(&mut EventCx::new(&mut args))`, a test).
    pub fn new(args: &'a mut A) -> Self {
        Self { args, sink: None, event: None, origin: None }
    }

    /// Args whose base behaviour also raises to `sink`.
    pub fn with_sink(args: &'a mut A, sink: &'a mut dyn RaiseSink) -> Self {
        Self { args, sink: Some(sink), event: None, origin: None }
    }

    /// Names the event being delivered (its attribute name, `"OnSelectionChanged"`): the base
    /// behaviour raises under this name rather than the method's own default. Set by
    /// [`super::Control::dispatch_event`] so a level method shared by several events
    /// (`ListControl::on_selection_changed`) raises the one that was delivered.
    pub fn named(mut self, event: &'static str) -> Self {
        self.event = Some(event);
        self
    }

    /// The class the event is delivered to (the outermost object): the sender's element name when
    /// there is no sink, even if a base class ends up raising it. Set by
    /// [`super::Control::dispatch_event`] and the provided operations (`perform_click`…).
    pub fn from_class(mut self, class: &'static str) -> Self {
        self.origin.get_or_insert(class);
        self
    }

    /// [`Self::from_class`] on a borrowed context.
    pub fn set_origin(&mut self, class: &'static str) {
        self.origin.get_or_insert(class);
    }

    /// The event being delivered, when the caller named it.
    pub fn event_name(&self) -> Option<&'static str> {
        self.event
    }

    /// Whether the base behaviour also raises to a sink (a `.kbview` element).
    pub fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    /// The sender the sink describes (`None` without a sink).
    pub fn sender(&self) -> Option<ElementRef<'_>> {
        self.sink.as_deref().map(|s| s.sender())
    }

    /// The args.
    pub fn args(&self) -> &A {
        self.args
    }

    /// The args, mutably (`handled`, `cancel`).
    pub fn args_mut(&mut self) -> &mut A {
        self.args
    }

    /// A shorter-lived copy of this context (the same args and sink).
    pub fn reborrow(&mut self) -> EventCx<'_, A> {
        EventCx { args: &mut *self.args, sink: reborrow_sink(&mut self.sink), event: self.event, origin: self.origin }
    }

    /// Raises the event to the sink, then to `owner`'s Rust subscribers — what every default
    /// `on_…` method ends with. `default_event` is the attribute name raised when the caller
    /// did not name the event ([`EventCx::named`]). A handler that marks the args handled
    /// stops the raise, as in [`crate::events::Event::raise`].
    pub fn raise<C: Component + ?Sized>(&mut self, owner: &C, default_event: &'static str) {
        let event = self.event.unwrap_or(default_event);
        let args: &mut dyn EventArgs = self.args.as_dyn_args();
        raise_to(owner.as_component(), self.origin, event, args, &mut self.sink);
    }
}

impl<'a, A: EventArgs> EventCx<'a, A> {
    /// The same context over `&mut dyn EventArgs` (what [`super::Control::on_event`] takes).
    pub fn as_dyn(&mut self) -> EventCx<'_, dyn EventArgs> {
        EventCx { args: &mut *self.args, sink: reborrow_sink(&mut self.sink), event: self.event, origin: self.origin }
    }
}

impl<'a> EventCx<'a, dyn EventArgs> {
    /// The same context with the args downcast to `B`, `None` when they are another type.
    pub fn typed<B: EventArgs>(&mut self) -> Option<EventCx<'_, B>> {
        let (event, origin) = (self.event, self.origin);
        let sink = reborrow_sink(&mut self.sink);
        let args = self.args.downcast_mut::<B>()?;
        Some(EventCx { args, sink, event, origin })
    }
}

impl<A: ?Sized> Deref for EventCx<'_, A> {
    type Target = A;
    fn deref(&self) -> &A {
        self.args
    }
}

impl<A: ?Sized> DerefMut for EventCx<'_, A> {
    fn deref_mut(&mut self) -> &mut A {
        self.args
    }
}

/// `Option<&mut dyn RaiseSink>` reborrowed for a shorter lifetime.
fn reborrow_sink<'b>(sink: &'b mut Option<&mut dyn RaiseSink>) -> Option<&'b mut dyn RaiseSink> {
    match sink {
        Some(s) => Some(&mut **s),
        None => None,
    }
}

/// `&mut A` → `&mut dyn EventArgs` for a sized `A` and for `dyn EventArgs` itself.
#[doc(hidden)]
pub trait AsDynArgs {
    fn as_dyn_args(&mut self) -> &mut dyn EventArgs;
}

impl<A: EventArgs> AsDynArgs for A {
    fn as_dyn_args(&mut self) -> &mut dyn EventArgs {
        self
    }
}

impl AsDynArgs for dyn EventArgs {
    fn as_dyn_args(&mut self) -> &mut dyn EventArgs {
        self
    }
}

/// The one raise: the sink first (the `.kbview` handler — WinForms' designer-generated
/// subscription, made in `InitializeComponent`, runs first), then the Rust subscribers, unless the
/// sink's handler marked the args handled.
fn raise_to(owner: &dyn Component, origin: Option<&'static str>, event: &'static str, args: &mut dyn EventArgs, sink: &mut Option<&mut dyn RaiseSink>) {
    if let Some(s) = sink.as_deref_mut() {
        s.raise(event, args);
        if args.as_handled().is_some_and(|h| h.handled()) {
            return;
        }
    }
    let events = &owner.component_core().events;
    if !events.has_subscribers(event) {
        return;
    }
    match sink.as_deref() {
        Some(s) => events.raise(event, &s.sender(), args),
        None => {
            let name = owner.display_name();
            let bounds = owner.as_control().map(|c| c.control_core().bounds).unwrap_or_default();
            let focus_id = owner.as_control().and_then(|c| c.control_core().focus_id);
            let sender = ElementRef { name: Some(name), element: origin.unwrap_or(owner.class_name()), id: "", bounds, focus_id, attributes: &[] };
            events.raise(event, &sender, args);
        }
    }
}

/// The argument of `on_paint` / `on_paint_background` / `on_print`: the drawing surface (WinForms'
/// `e.Graphics`: a [`Graphics`] — lines, rectangles, ellipses, arcs, paths, gradients, pens, text
/// layout, images, clip, transforms, saved states), the rectangle to paint (`e.ClipRectangle`, the
/// control's bounds in the surface's coordinates), and the interaction state the host observed.
///
/// `e.graphics` also answers the Kubuno canvas primitives (`fill_rounded`, `text_ellipsis`…, the
/// [`Canvas`](kubuno_ui::Canvas) trait), so a `kubuno_ui` widget paints through it: everything drawn through it is
/// part of the control's paint buffer (see `crate::component::paint`). The raw canvas
/// ([`PaintEventCx::canvas`]) is there for what needs the control surface itself; drawing on it
/// directly bypasses the buffer (the control is then repainted every frame).
pub struct PaintEventCx<'a> {
    /// The surface (`e.Graphics`).
    pub graphics: &'a Graphics<'a>,
    canvas: &'a dyn ControlCanvas,
    /// Where to paint (`e.ClipRectangle`): the control's bounds.
    pub clip_rectangle: Rect,
    /// Hover / pressed / focused / disabled, as the host observed them.
    pub state: WidgetState,
    sink: Option<&'a mut dyn RaiseSink>,
}

impl<'a> PaintEventCx<'a> {
    pub fn new(graphics: &'a Graphics<'a>, canvas: &'a dyn ControlCanvas, clip_rectangle: Rect, state: WidgetState) -> Self {
        Self { graphics, canvas, clip_rectangle, state, sink: None }
    }

    /// Also raises `Paint` to `sink`.
    pub fn with_sink(mut self, sink: &'a mut dyn RaiseSink) -> Self {
        self.sink = Some(sink);
        self
    }

    /// The surface (`e.Graphics`).
    pub fn graphics(&self) -> &'a Graphics<'a> {
        self.graphics
    }

    /// The control canvas underneath (system visuals, themed parts). What is drawn on it directly is
    /// not recorded in the control's paint buffer, so asking for it turns the buffer off for this
    /// paint.
    pub fn canvas(&self) -> &'a dyn ControlCanvas {
        let _ = self.graphics.raw_canvas();
        self.canvas
    }

    /// The rectangle to paint (the control's bounds, surface coordinates).
    pub fn bounds(&self) -> Rect {
        self.clip_rectangle
    }

    /// The control's client area in its own coordinates: `(0, 0, width, height)` — draw with it
    /// after `e.graphics.translate_transform(e.bounds().left, e.bounds().top)`.
    pub fn client_rectangle(&self) -> Rect {
        let r = self.clip_rectangle;
        Rect::new(0.0, 0.0, (r.right - r.left).max(0.0), (r.bottom - r.top).max(0.0))
    }

    /// Whether this paint raises `Paint` to a `.kbview` handler.
    pub fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    /// A shorter-lived copy (the same surface, rectangle, state and sink).
    pub fn reborrow(&mut self) -> PaintEventCx<'_> {
        PaintEventCx { graphics: self.graphics, canvas: self.canvas, clip_rectangle: self.clip_rectangle, state: self.state, sink: reborrow_sink(&mut self.sink) }
    }

    /// Raises `Paint` (its [`PaintEventArgs`], lending the surface) to the sink and `owner`'s Rust
    /// subscribers — the end of the default `on_paint`.
    pub fn raise<C: Component + ?Sized>(&mut self, owner: &C, event: &'static str) {
        let graphics = self.graphics;
        let clip = self.clip_rectangle;
        let sink = &mut self.sink;
        PaintEventArgs::lend(graphics, clip, |args| raise_to(owner.as_component(), None, event, args, sink));
    }
}
