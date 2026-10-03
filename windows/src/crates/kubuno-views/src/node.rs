//! The live tree: [`ViewNode`], the trait every compiled `.kbview` element
//! becomes, and the five concrete nodes `crate::registry::components`'
//! `build` closures construct.
//!
//! ## Why this is not just `kubuno_ui::Widget`
//!
//! `kubuno_ui::Widget::paint` takes one [`kubuno_ui::WidgetState`] the
//! *caller* computed — a container's own `Widget` impl (`Panel::paint`,
//! `Card::paint`) applies exactly one state (`WidgetState::REST`, `disabled`
//! aside) to *every* child, because a generic container has no way to know
//! which child the pointer is over (see `containers.rs`'s own comment on
//! `paint_children_unclipped`: "the caller that tracks hover passes it to the
//! child itself"). That is correct for `kubuno_ui`'s existing callers, which
//! hand-write their own hit-testing per screen (`shell/src/admin_users.rs`'s
//! `DataTable`) — but it means routing `<Button OnClick="…">` through
//! `Card`/`Stack`'s own `Widget::paint` would silently paint every button at
//! rest and fire no clicks at all.
//!
//! So a [`ViewNode`] container (`CardNode`, `StackNode`) does not delegate to
//! `Panel`'s child-painting: it uses the real `kubuno_ui` container **for its
//! own chrome and layout arithmetic only** (`Card::paint_body`'s body
//! closure, `Stack::layout_children`) and then recurses into its own
//! [`ViewNode`] children itself, computing each one's `hot`/`pressed`/
//! `focused` from the live [`Frame`] — the same per-child interactivity
//! `admin_users.rs` hand-writes today, generalised once instead of per
//! screen.
//!
//! ## Rebuilt every frame, state kept where it must persist
//!
//! Per §0, the concrete `kubuno_ui` widget (`Button`, `Switch`…) a node wraps
//! is cheap to construct and is rebuilt fresh every frame from
//! [`crate::binding::PropSource`] — there is nothing to keep. Two things
//! *do* persist on the node itself, because losing them would be visibly
//! wrong: a leaf's `pressed` flag (press-then-release-inside is how a click
//! is recognised; see [`ButtonNode::paint`]) and [`TextFieldNode`]'s actual
//! `kubuno_ui::text::TextField`, whose caret/selection/undo history is real
//! state a rebuild-from-nothing would erase on every frame.

use kubuno_controls::host::Frame;
use kubuno_controls::{layout_panels::FlowDirection, ControlCanvas, Padding};
use kubuno_ui::buttons::{Button, Size as ButtonSize, Switch, SwitchSize, Variant};
use kubuno_ui::containers::{Card, Stack, Surface};
use kubuno_ui::text::{EditInput, TextField as UiTextField};
use kubuno_ui::{Canvas, FocusId, FocusOpts, FocusRing, Rect, Size, Widget, WidgetState};

use std::fmt;
use std::rc::Rc;

use crate::binding::{HandlerTable, PropSource, Value, ViewModel};
#[cfg(test)]
use crate::binding::BindingMode;
use crate::component::{Control, EventCx, PaintEventCx, RaiseSink};

/// The nodes of application classes and of the built-in non-visual components (EVT-7b).
pub mod custom;
use crate::events::router::{InputRouter, SlotEvents};
use crate::events::{ChangeSource, CheckedChangedEventArgs, ElementRef, EventArgs, TextChangedEventArgs};

/// One interaction a frame produced, in `RibbonEvent`'s own shape (§2: "a
/// generic interpreter just needs to route those by name instead of a
/// hand-written `match`"): a boxed [`crate::runtime::Runtime::frame`] call
/// hands the caller a `Vec<ViewEvent>` in addition to dispatching any
/// matching named handler itself, so a caller with no handler table can still
/// react.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewEvent {
    /// The `x:Name`-derived id of the node that raised it, when it has one.
    pub focus_id: Option<FocusId>,
    /// The `On*="handler_name"` attribute's value, when the element declared
    /// one — `None` for a two-way-bound control with no explicit handler.
    pub handler: Option<String>,
    pub kind: ViewEventKind,
}

/// What happened. The first three are the historical payloads (unchanged, so a caller
/// matching on them keeps working); every event of the typed event system
/// (`vskubuno/docs/EVENTS.md` §5.4, raised by [`crate::events::router`]: MouseDown,
/// KeyPress, GotFocus, Load…) arrives as [`ViewEventKind::Other`], and only when the
/// element names a handler for it.
#[derive(Clone)]
pub enum ViewEventKind {
    Clicked,
    Toggled(bool),
    Changed(String),
    /// A typed event: its display name (`"MouseDown"`) and its args, after the handler ran.
    Other { name: &'static str, args: Rc<dyn EventArgs> },
}

impl fmt::Debug for ViewEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewEventKind::Clicked => f.write_str("Clicked"),
            ViewEventKind::Toggled(on) => f.debug_tuple("Toggled").field(on).finish(),
            ViewEventKind::Changed(s) => f.debug_tuple("Changed").field(s).finish(),
            ViewEventKind::Other { name, args } => f
                .debug_struct("Other")
                .field("name", name)
                .field("args", &args.type_chain().first().copied().unwrap_or("EventArgs"))
                .finish(),
        }
    }
}

/// `Other` events compare by name and by identity of their args (the args are not
/// comparable in general).
impl PartialEq for ViewEventKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ViewEventKind::Clicked, ViewEventKind::Clicked) => true,
            (ViewEventKind::Toggled(a), ViewEventKind::Toggled(b)) => a == b,
            (ViewEventKind::Changed(a), ViewEventKind::Changed(b)) => a == b,
            (ViewEventKind::Other { name: a, args: x }, ViewEventKind::Other { name: b, args: y }) => a == b && Rc::ptr_eq(x, y),
            _ => false,
        }
    }
}

/// The element currently painting (set by `crate::design::DesignSlot` around its node):
/// the sender of the events its node raises, and where it was painted.
pub(crate) type CurrentElement = Option<(Rc<SlotEvents>, Rect)>;

/// What a [`ViewNode`] needs to paint one frame: the live canvas, the frame's
/// input, the view model bindings read from and write to, the focus ring
/// leaves register with, the named handler table, and the event sink. Built
/// once per frame by [`crate::runtime::Runtime::frame`] and reborrowed down
/// the tree (see [`PaintCx::reborrow`]) — a container cannot simply pass its
/// own `&mut PaintCx` to more than one child, so each recursion borrows a
/// fresh, field-disjoint copy.
pub struct PaintCx<'a> {
    pub canvas: &'a dyn ControlCanvas,
    pub frame: &'a Frame,
    pub vm: &'a mut dyn ViewModel,
    pub focus: &'a mut FocusRing,
    pub handlers: &'a mut HandlerTable,
    pub events: &'a mut Vec<ViewEvent>,
    /// The frame's layout map (`vskubuno/docs/DESIGNER.md` §6, DSG-6) —
    /// `None` outside design mode, so recording costs one branch and no
    /// allocation for every ordinary (non-designer) frame. Populated by
    /// [`crate::design::DesignSlot::paint`], which every compiled element is
    /// wrapped in (`crate::compile::build_node`); nothing else in this
    /// module reads or writes it.
    pub design: Option<&'a mut crate::design::LayoutMap>,
    /// The view's input router (EVT-2), which every `DesignSlot` registers its element
    /// with while painting; `None` when painting outside a `Runtime` frame.
    pub(crate) router: Option<&'a mut InputRouter>,
    /// See [`CurrentElement`].
    pub(crate) sender: CurrentElement,
    /// The class instance of the element painting (EVT-7a, set by `crate::design::DesignSlot`):
    /// its node's events are delivered through its `on_…` methods.
    pub(crate) control: Option<&'a mut dyn Control>,
    /// What the elements ask of the window this frame (pointer shape, tooltip, accessibility tree,
    /// mnemonics…, `crate::common`); `None` outside a runtime frame.
    pub(crate) services: Option<&'a mut crate::common::FrameServices>,
    /// The element painting was activated by its mnemonic (Alt + its letter): its node acts as if
    /// it had been clicked. Set by `crate::design::DesignSlot` for its own node only.
    pub(crate) activate: bool,
}

impl<'a> PaintCx<'a> {
    /// A fresh [`PaintCx`] borrowing the same underlying `vm`/`focus`/
    /// `handlers`/`events`/`design` — what a container hands each child in
    /// turn, since `self`'s own fields cannot be moved twice.
    pub fn reborrow(&mut self) -> PaintCx<'_> {
        PaintCx {
            canvas: self.canvas,
            frame: self.frame,
            vm: &mut *self.vm,
            focus: &mut *self.focus,
            handlers: &mut *self.handlers,
            events: &mut *self.events,
            design: self.design.as_deref_mut(),
            router: self.router.as_deref_mut(),
            sender: self.sender.clone(),
            control: reborrow_control(&mut self.control),
            services: self.services.as_deref_mut(),
            activate: false,
        }
    }

    /// The text a control shows for `text` with `UseMnemonic` (see `crate::common::mnemonic_text`):
    /// the ampersands removed, the shortcut registered for the element painting, and the index of
    /// the letter to underline while Alt is held.
    pub(crate) fn mnemonic_text(&mut self, text: &str, use_mnemonic: bool, action: crate::common::MnemonicAction) -> (String, Option<usize>) {
        let element = self.sender.as_ref().map(|(slot, _)| slot.id.clone());
        crate::common::mnemonic_text(text, use_mnemonic, self.frame, self.services.as_deref_mut(), element.as_deref(), action)
    }

    /// [`Self::reborrow`] with another canvas and frame — what a container that paints
    /// its content through its own transformed canvas/frame (a scroll area's viewport)
    /// hands its child.
    pub fn with_surface<'b>(&'b mut self, canvas: &'b dyn ControlCanvas, frame: &'b Frame) -> PaintCx<'b> {
        let mut cx = self.reborrow();
        cx.canvas = canvas;
        cx.frame = frame;
        cx
    }

    /// The canvas-independent slice of `self` — what a leaf's `interact`
    /// (see [`InteractCx`]) actually needs. Borrowing it and then painting
    /// afterwards is how [`ButtonNode::paint`]/[`SwitchNode::paint`] get
    /// same-frame visual feedback from a binding write-back or a handler:
    /// `interact` runs (and may call `vm.set`) *before* the node re-resolves
    /// its bound properties to build the widget it actually paints.
    pub fn interact_cx(&mut self) -> InteractCx<'_> {
        InteractCx {
            frame: self.frame,
            vm: &mut *self.vm,
            focus: &mut *self.focus,
            handlers: &mut *self.handlers,
            events: &mut *self.events,
            sender: self.sender.clone(),
            control: reborrow_control(&mut self.control),
            activate: self.activate,
        }
    }

    /// Raises one of the node's own events (`vskubuno/docs/EVENTS.md` EVT-2, "typed
    /// fire"): dispatches the handler the element names with its typed `args` (the
    /// typed handler if the table has one, else the legacy handler with
    /// `args.legacy_value()` — exactly the value this event always produced), and
    /// records `kind` as a [`ViewEvent`], so both consumers §2 names (a hand-written
    /// `match` over returned events, or a `handlers!` table) see it. The sender is the
    /// element painting (see [`CurrentElement`]). `pub(crate)` so every component
    /// family (`registry::families::*`) raises events the same way.
    pub(crate) fn fire(&mut self, event: &'static str, focus_id: Option<FocusId>, handler: Option<&str>, kind: ViewEventKind, args: &mut dyn EventArgs) {
        let control = reborrow_control(&mut self.control);
        fire(self.handlers, self.events, self.vm, &self.sender, control, event, focus_id, handler, kind, args);
    }
}

/// What a leaf node's interaction step needs: the frame's input, the
/// bindings to read/write, the focus ring, the handler table and the event
/// sink — everything [`PaintCx`] carries *except* the canvas, so this half of
/// a frame can run (and be unit-tested — see `node::tests`) without a live
/// `kubuno_ui::Canvas`. Built from a [`PaintCx`] with [`PaintCx::interact_cx`].
pub struct InteractCx<'a> {
    pub frame: &'a Frame,
    pub vm: &'a mut dyn ViewModel,
    pub focus: &'a mut FocusRing,
    pub handlers: &'a mut HandlerTable,
    pub events: &'a mut Vec<ViewEvent>,
    /// See [`CurrentElement`].
    pub(crate) sender: CurrentElement,
    /// See [`PaintCx::control`].
    pub(crate) control: Option<&'a mut dyn Control>,
    /// See [`PaintCx::activate`]: a node treats it as a click.
    pub(crate) activate: bool,
}

impl<'a> InteractCx<'a> {
    /// An interaction context with no current element (tests).
    #[cfg(test)]
    pub(crate) fn new(
        frame: &'a Frame,
        vm: &'a mut dyn ViewModel,
        focus: &'a mut FocusRing,
        handlers: &'a mut HandlerTable,
        events: &'a mut Vec<ViewEvent>,
    ) -> Self {
        Self { frame, vm, focus, handlers, events, sender: None, control: None, activate: false }
    }

    /// See [`PaintCx::fire`] — the same dispatch, from the canvas-free side.
    /// `pub(crate)` for the same reason as [`PaintCx::fire`].
    pub(crate) fn fire(&mut self, event: &'static str, focus_id: Option<FocusId>, handler: Option<&str>, kind: ViewEventKind, args: &mut dyn EventArgs) {
        let control = reborrow_control(&mut self.control);
        fire(self.handlers, self.events, self.vm, &self.sender, control, event, focus_id, handler, kind, args);
    }
}

/// `Option<&mut dyn Control>` reborrowed for a shorter lifetime (what a context hands the
/// context it derives).
pub(crate) fn reborrow_control<'b>(control: &'b mut Option<&mut dyn Control>) -> Option<&'b mut dyn Control> {
    match control {
        Some(c) => Some(&mut **c),
        None => None,
    }
}

/// The one implementation [`PaintCx::fire`]/[`InteractCx::fire`] both forward
/// to, so the two contexts cannot drift on what "firing an event" means.
///
/// With the element's control (EVT-7a), the event (`event`, its attribute name) is delivered
/// through the control's `on_…` method; its base behaviour raises it to [`FireSink`] — the
/// handler, then the reported [`ViewEvent`] — and to the control's Rust subscribers. An override
/// that does not call its base suppresses both.
#[allow(clippy::too_many_arguments)]
fn fire(
    handlers: &mut HandlerTable,
    events: &mut Vec<ViewEvent>,
    vm: &mut dyn ViewModel,
    sender: &CurrentElement,
    control: Option<&mut dyn Control>,
    event: &'static str,
    focus_id: Option<FocusId>,
    handler: Option<&str>,
    kind: ViewEventKind,
    args: &mut dyn EventArgs,
) {
    if takes_pointer(&kind) {
        POINTER_HANDLED.with(|c| c.set(c.get().wrapping_add(1)));
    }
    if let Some(control) = control {
        let mut sink = FireSink { handlers, events, vm, sender, focus_id, handler, kind: Some(kind) };
        control.dispatch_event(event, &mut EventCx::with_sink(args, &mut sink));
        return;
    }
    if let Some(name) = handler {
        let sender = match sender {
            Some((slot, bounds)) => slot.sender(*bounds),
            None => ElementRef { name: None, element: "", id: "", bounds: Rect::default(), focus_id, attributes: &[] },
        };
        handlers.dispatch_args(name, vm, &sender, args);
    }
    events.push(ViewEvent { focus_id, handler: handler.map(str::to_string), kind });
}

/// The [`RaiseSink`] of a node's own event: the first raise dispatches the handler the element
/// names and reports the event, exactly as `fire` does without a control.
struct FireSink<'s> {
    handlers: &'s mut HandlerTable,
    events: &'s mut Vec<ViewEvent>,
    vm: &'s mut dyn ViewModel,
    sender: &'s CurrentElement,
    focus_id: Option<FocusId>,
    handler: Option<&'s str>,
    kind: Option<ViewEventKind>,
}

impl RaiseSink for FireSink<'_> {
    fn sender(&self) -> ElementRef<'_> {
        match self.sender {
            Some((slot, bounds)) => slot.sender(*bounds),
            None => ElementRef { name: None, element: "", id: "", bounds: Rect::default(), focus_id: self.focus_id, attributes: &[] },
        }
    }

    fn raise(&mut self, _event: &'static str, args: &mut dyn EventArgs) {
        let Some(kind) = self.kind.take() else { return };
        if let Some(name) = self.handler {
            let sender = match self.sender {
                Some((slot, bounds)) => slot.sender(*bounds),
                None => ElementRef { name: None, element: "", id: "", bounds: Rect::default(), focus_id: self.focus_id, attributes: &[] },
            };
            self.handlers.dispatch_args(name, &mut *self.vm, &sender, args);
        }
        self.events.push(ViewEvent { focus_id: self.focus_id, handler: self.handler.map(str::to_string), kind });
    }
}

/// The [`RaiseSink`] of a paint-time event (`Paint`, `DrawItem`, `MeasureItem`): the element's
/// handler runs, and — raised for every paint or item — nothing is reported in the frame's events.
pub(crate) struct QuietSink<'s> {
    pub handlers: &'s mut HandlerTable,
    pub vm: &'s mut dyn ViewModel,
    pub sender: &'s CurrentElement,
    pub focus_id: Option<FocusId>,
    pub handler: Option<&'s str>,
}

impl RaiseSink for QuietSink<'_> {
    fn sender(&self) -> ElementRef<'_> {
        match self.sender {
            Some((slot, bounds)) => slot.sender(*bounds),
            None => ElementRef { name: None, element: "", id: "", bounds: Rect::default(), focus_id: self.focus_id, attributes: &[] },
        }
    }

    fn raise(&mut self, _event: &'static str, args: &mut dyn EventArgs) {
        if let Some(name) = self.handler {
            let sender = match self.sender {
                Some((slot, bounds)) => slot.sender(*bounds),
                None => ElementRef { name: None, element: "", id: "", bounds: Rect::default(), focus_id: self.focus_id, attributes: &[] },
            };
            self.handlers.dispatch_args(name, &mut *self.vm, &sender, args);
        }
    }
}

impl<'a> PaintCx<'a> {
    /// Raises a paint-time event (`OnDrawItem`, `OnMeasureItem`, `OnPaint`) of the element painting:
    /// through its class's `on_…` method when it has one (`on_event`), then to its handler — never
    /// reported in the frame's events (it is raised for every paint or item).
    pub(crate) fn fire_quiet(&mut self, event: &'static str, handler: Option<&str>, args: &mut dyn EventArgs) {
        let focus_id = self.sender.as_ref().and_then(|(slot, _)| slot.focus_id);
        let mut sink = QuietSink { handlers: &mut *self.handlers, vm: &mut *self.vm, sender: &self.sender, focus_id, handler };
        match self.control.as_deref_mut() {
            Some(control) => control.dispatch_event(event, &mut EventCx::with_sink(args, &mut sink)),
            None => sink.raise(event, args),
        }
    }
}

/// A compiled `.kbview` element: something that can size itself and paint
/// itself (and its children, for a container) into a live frame. See the
/// module doc for why this is not simply `kubuno_ui::Widget`.
pub trait ViewNode {
    /// The node's own desired size — a leaf's is its `kubuno_ui::Widget`
    /// measurement; a container's folds its children's (see
    /// [`StackNode::measure`]).
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size;

    /// [`Self::measure`], told how much width it is actually about to get.
    /// Defaults to [`Self::measure`]'s own unconstrained answer — exactly
    /// right for a leaf whose height does not depend on its width. Override
    /// this only for a widget whose `kubuno_ui::Widget::measure` itself
    /// documents "no intrinsic width" and derives its height by wrapping
    /// content into an assumed width (`Callout`'s own designer-width default,
    /// for one) — that assumed width is almost never the real one a `<Stack>`
    /// column is about to give it (`StackNode`
    /// hands every block the FULL row width — `kubuno_ui::containers::Stack::
    /// column`'s own "each block as wide as the client area" design), so
    /// without this the reported height is wrong, sometimes wildly so.
    fn measure_for_width(&self, c: &dyn Canvas, vm: &dyn ViewModel, _width: f32) -> Size {
        self.measure(c, vm)
    }

    /// The width this node wants for ITSELF when it is a direct child of a
    /// `<Stack>` column — `None` (the default) keeps the full row width
    /// `StackNode` otherwise hands every
    /// block, correct for a widget that already limits itself within
    /// whatever bounds it paints into (a checkbox, a switch…) or that
    /// legitimately has no intrinsic width and should span the row
    /// (`Separator`, `ProgressBar` — both documented `w-full` in
    /// `kubuno-ui`). `Some(width)` narrows the block to exactly that many
    /// DIP, left-aligned — what a fixed-content-size widget like `Badge`/
    /// `IconButton` needs: their own `kubuno_ui::Widget::paint` fills
    /// whatever rectangle it is handed, by design (documented on `Badge`'s
    /// own `paint`: "When the caller sized the badge from `measure` this is
    /// identical to laying it out from the left"), so the CALLER — this
    /// crate's own node — must size that rectangle first.
    fn intrinsic_width(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Option<f32> {
        None
    }

    /// Whether the node paints nothing of its own around its content (a container with no
    /// border and no background), so the designer draws a faint dashed outline around it to
    /// keep it findable and droppable, like the dotted border Windows Forms shows around a
    /// borderless `Panel`. `false` (the default) for anything visible on its own: a leaf control
    /// always looks exactly as it does at run time.
    fn is_invisible_container(&self, _vm: &dyn ViewModel) -> bool {
        false
    }

    /// Whether the element is hidden this frame (`Visible="false"` at run time): a container lays it
    /// out like WinForms lays out an invisible control — a hidden docked child takes no band.
    fn is_hidden(&self, _vm: &dyn ViewModel) -> bool {
        false
    }

    /// Whether the node paints other elements inside its box that must be clipped to it although its element
    /// takes no children in the document: a user control's own view (`crate::clip`). A container element (its
    /// registry entry takes children) always clips them; `false` (the default) for everything else.
    fn clips_children(&self) -> bool {
        false
    }

    /// Lays out (a container) and paints into `bounds`, reading bindings from
    /// `cx.vm` and reporting interaction through `cx` (see [`PaintCx::fire`]).
    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect);
}

/// The args of a control's own mouse Click (a `<Button>`'s, EVT-4): the left button, the
/// frame's click count, the pointer relative to the control's `bounds` and the modifiers. Click
/// carries `MouseEventArgs` (WinForms passes a `MouseEventArgs` typed as `EventArgs`), so the
/// designer's `fn …_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs)` stub fits it.
pub(crate) fn click_args(frame: &Frame, bounds: Rect) -> crate::events::MouseEventArgs {
    crate::events::MouseEventArgs {
        button: crate::events::MouseButton::Left,
        clicks: frame.click_count.max(1),
        x: frame.mouse.0 - bounds.left,
        y: frame.mouse.1 - bounds.top,
        delta: 0.0,
        mods: frame.mods,
    }
}

/// A rectangle-based click: `true` exactly once, the frame the pointer is
/// released while still over the target *and* it went down over the target
/// too (a press-then-drag-off-then-release is not a click, matching every
/// desktop toolkit's own `perform_click`). `pressed` is the node's own
/// persisted flag, updated in place. `pub(crate)` so every component family
/// shares this one implementation instead of each keeping its own verbatim
/// copy (as `families::display`/`choice`/`containers`/`text` used to,
/// documented at each site as "mirrors `crate::node`'s own (private)
/// `press_release` exactly").
pub(crate) fn press_release(pressed: &mut bool, hot: bool, mouse_down: bool) -> (bool, bool) {
    let down_now = hot && mouse_down;
    let clicked = *pressed && hot && !mouse_down;
    *pressed = down_now;
    (down_now, clicked)
}

// ─────────────────────────────────────────────────────────────────────────
// Button
// ─────────────────────────────────────────────────────────────────────────

pub struct ButtonNode {
    pub text: PropSource<String>,
    pub variant: PropSource<String>,
    pub size: PropSource<String>,
    pub icon: PropSource<String>,
    pub loading: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_click: Option<String>,
    /// The `ButtonBase` properties (`TextAlign`, `Image`, `ImageAlign`, `TextImageRelation`,
    /// `UseMnemonic`).
    pub base: crate::common::ButtonBaseProps,
    /// `DropDownMenu`: the `<ContextMenu>` a click opens below the button.
    pub drop_down: Option<String>,
    pressed: bool,
}

impl ButtonNode {
    pub fn new(
        text: PropSource<String>,
        variant: PropSource<String>,
        size: PropSource<String>,
        icon: PropSource<String>,
        loading: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_click: Option<String>,
    ) -> Self {
        Self { text, variant, size, icon, loading, focus_id, on_click, base: Default::default(), drop_down: None, pressed: false }
    }

    /// Builder: the `<ContextMenu>` a click opens below the button (`DropDownMenu`).
    pub fn with_drop_down(mut self, menu: Option<String>) -> Self {
        self.drop_down = menu.filter(|m| !m.trim().is_empty());
        self
    }

    /// Builder: the `ButtonBase` properties.
    pub fn with_base(mut self, base: crate::common::ButtonBaseProps) -> Self {
        self.base = base;
        self
    }

    /// The text shown: the `&` of a mnemonic removed (`UseMnemonic`).
    fn shown_text(&self, vm: &dyn ViewModel) -> String {
        let text = self.text.resolve(vm);
        if self.base.use_mnemonic { crate::common::mnemonic(&text).0 } else { text }
    }

    fn build(&self, vm: &dyn ViewModel) -> Button {
        let mut b = Button::new(&self.shown_text(vm)).variant(parse_variant(&self.variant.resolve(vm))).size(parse_button_size(&self.size.resolve(vm)));
        if let Some(name) = crate::icon::resolve(&self.icon.resolve(vm)) {
            b = b.icon(name);
        }
        // Without an `Icon`, the `Image` is drawn by the icon pipeline (SVG, every raster format, a
        // resource), at its own size unless `IconSize` says otherwise, placed like WinForms places it.
        let has_icon = b.icon.is_some();
        let image_icon = if has_icon { None } else { self.image_icon() };
        if let Some((name, _)) = image_icon {
            b = b.icon(name);
        }
        let mut b = b.loading(self.loading.resolve(vm));
        b.text_align = self.base.text_align;
        b.image_align = self.base.image_align;
        b.text_image_relation = self.base.relation;
        // The icon: its own size and spacing, and laid out like an image when `ImageAlign` or
        // `TextImageRelation` is written — before the text unless a relation says otherwise.
        b.icon_size = self.base.icon_size.or(image_icon.map(|(_, natural)| natural));
        b.gap = self.base.icon_spacing;
        b.icon_aligned = self.base.icon_aligned || image_icon.is_some();
        if has_icon && !self.base.relation_written {
            b.text_image_relation = kubuno_controls::buttons::TextImageRelation::ImageBeforeText;
        }
        b
    }

    /// The `Image` as an icon (with its own size in DIP), when it names an image the icon pipeline
    /// draws.
    fn image_icon(&self) -> Option<(&'static str, f32)> {
        let name = crate::icon::resolve(self.base.image.as_deref()?)?;
        let spec = drive_app_controls::icon_source::parse(name);
        // Its own size, but an icon file (several sizes, up to 256) gives its 32 px one, and nothing is
        // larger than 48: a button is not a picture box.
        let is_ico = drive_app_controls::icon_source::image_extension(spec.source) == Some("ico") || kubuno_controls::icon_image::has_several_sizes(spec.source);
        let natural = if is_ico { 32.0 } else { kubuno_controls::icon_image::image_size(spec.source).map_or(16.0, |(w, h)| w.max(h)).min(48.0) };
        Some((name, natural))
    }

    /// Writes the resolved properties into the element's [`crate::controls::Button`] (what
    /// [`Self::build`] gives the widget), without raising its change events.
    fn sync(&self, button: &mut crate::controls::Button, vm: &dyn ViewModel, mnemonic: Option<usize>) {
        use crate::component::{HasButtonBaseCore, HasControlCore};
        let text = self.shown_text(vm);
        if button.control_core().text != text {
            button.control_core_mut().text = text;
        }
        button.variant = parse_variant(&self.variant.resolve(vm));
        button.size = parse_button_size(&self.size.resolve(vm));
        button.icon = crate::icon::resolve(&self.icon.resolve(vm));
        let has_icon = button.icon.is_some();
        // Without an `Icon`, the `Image` is drawn as the icon (see `Self::build`).
        let image_icon = if has_icon { None } else { self.image_icon() };
        if let Some((name, _)) = image_icon {
            button.icon = Some(name);
        }
        button.loading = self.loading.resolve(vm);
        button.icon_size = self.base.icon_size.or(image_icon.map(|(_, natural)| natural));
        button.icon_spacing = self.base.icon_spacing;
        button.icon_aligned = self.base.icon_aligned || image_icon.is_some();
        let icon_first = has_icon && !self.base.relation_written;
        let core = button.button_base_core_mut();
        core.text_align = self.base.text_align;
        core.image = if has_icon { self.base.image.clone() } else { None };
        core.image_align = self.base.image_align;
        // An icon goes before the text unless a relation is written (see `Self::build`).
        core.text_image_relation = if icon_first { kubuno_controls::buttons::TextImageRelation::ImageBeforeText } else { self.base.relation };
        core.use_mnemonic = self.base.use_mnemonic;
        core.mnemonic = mnemonic;
    }

    /// The canvas-independent half of a frame: hit-tests `bounds` against
    /// `ix.frame`'s pointer, tracks press/release (see [`press_release`]),
    /// and — on a completed click — dispatches `OnClick` and records a
    /// [`ViewEvent`]. Returns the [`WidgetState`] the caller should paint
    /// with. `pub(crate)` so this crate's own tests exercise click → handler
    /// dispatch without a live `Canvas` (see `node::tests`); the sole
    /// production caller is [`ButtonNode::paint`].
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let btn = self.build(ix.vm);
        let hot = !ix.frame.pointer_outside() && btn.hit_test(bounds, ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        // Its mnemonic (Alt + its letter) or assistive technology pressed it: a click.
        let clicked = clicked || ix.activate;
        if clicked {
            ix.fire("OnClick", self.focus_id, self.on_click.as_deref(), ViewEventKind::Clicked, &mut crate::node::click_args(ix.frame, bounds));
            if let Some(menu) = &self.drop_down {
                crate::window::show_context_menu(menu, crate::window::MenuAnchor::Below(crate::common::to_client(bounds)));
            }
        }
        state
    }
}

impl ViewNode for ButtonNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        // Re-resolved AFTER `interact`: a click's handler may have just
        // written through `ix.vm` (e.g. `Loading="{Binding …}"` flipped by
        // the very handler this click fired), and this is what gives that
        // write same-frame visual feedback instead of a one-frame lag.
        //
        // The mnemonic (`&Save`): registered for Alt + its letter, underlined while Alt is held.
        let raw = self.text.resolve(cx.vm);
        let (_, underline) = cx.mnemonic_text(&raw, self.base.use_mnemonic, crate::common::MnemonicAction::Activate);
        // The element's class instance (EVT-7a) — a `controls::Button`, or a class extending it —
        // gets the resolved properties and paints through its `on_paint` (an override draws
        // instead). Without one, the widget is painted directly, as before.
        // A class that clears `USER_PAINT` leaves the painting to the built-in look (WinForms: the
        // system paints a control that does not paint itself).
        if let Some(control) = cx.control.as_deref_mut().filter(|c| c.get_style(crate::component::ControlStyles::USER_PAINT)) {
            if let Some(button) = control.as_component_mut().find_base_mut::<crate::controls::Button>() {
                self.sync(button, &*cx.vm, underline);
                let g = kubuno_ui::graphics::Graphics::new(cx.canvas);
                // `OnPaint` on the element: raised with the surface lent, after the button's look.
                let handler = cx.sender.as_ref().and_then(|(slot, _)| slot.handler("OnPaint").map(str::to_string));
                let focus_id = self.focus_id;
                let mut sink = QuietSink { handlers: &mut *cx.handlers, vm: &mut *cx.vm, sender: &cx.sender, focus_id, handler: handler.as_deref() };
                control.on_paint(&mut PaintEventCx::new(&g, cx.canvas, bounds, state).with_sink(&mut sink));
                return;
            }
        }
        let mut btn = self.build(cx.vm);
        btn.mnemonic = underline;
        // The `Image` beside an `Icon` (without one, `Self::build` draws the image as the icon).
        let has_icon = crate::icon::resolve(&self.icon.resolve(cx.vm)).is_some();
        let image = self.base.image.as_deref().filter(|_| has_icon).and_then(|path| kubuno_controls::styled::load_image(cx.canvas, path));
        btn.image_size = image.as_ref().map(kubuno_controls::styled::image_size);
        btn.paint(cx.canvas, bounds, state);
        if let (Some(bitmap), Some(rect)) = (&image, btn.image_rect(cx.canvas, bounds)) {
            cx.canvas.draw_bitmap(bitmap, &rect, if state.disabled { 0.5 } else { 1.0 });
        }
    }
}

pub(crate) fn parse_variant(s: &str) -> Variant {
    match s {
        "Secondary" => Variant::Secondary,
        "Ghost" => Variant::Ghost,
        "Text" => Variant::Text,
        "Danger" => Variant::Danger,
        "TextDanger" => Variant::TextDanger,
        _ => Variant::Primary,
    }
}

pub(crate) fn parse_button_size(s: &str) -> ButtonSize {
    match s {
        "Sm" => ButtonSize::Sm,
        "Lg" => ButtonSize::Lg,
        _ => ButtonSize::Md,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Switch
// ─────────────────────────────────────────────────────────────────────────

pub struct SwitchNode {
    pub on: PropSource<bool>,
    pub label: PropSource<String>,
    pub description: PropSource<String>,
    pub size: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_toggled: Option<String>,
    /// `CheckAlign`: where the track sits against the label (WinForms `CheckBox.CheckAlign`):
    /// `MiddleLeft` before it (the default), `MiddleRight` at the box's right end after it.
    pub check_align: PropSource<String>,
    pressed: bool,
}

impl SwitchNode {
    pub fn new(
        on: PropSource<bool>,
        label: PropSource<String>,
        description: PropSource<String>,
        size: PropSource<String>,
        focus_id: Option<FocusId>,
        on_toggled: Option<String>,
    ) -> Self {
        Self { on, label, description, size, focus_id, on_toggled, check_align: PropSource::Literal("MiddleLeft".to_string()), pressed: false }
    }

    /// With `CheckAlign` (see the field).
    pub fn with_check_align(mut self, check_align: PropSource<String>) -> Self {
        self.check_align = check_align;
        self
    }

    fn build(&self, on: bool, vm: &dyn ViewModel) -> Switch {
        let mut sw = Switch::new()
            .on(on)
            .label(&crate::common::mnemonic(&self.label.resolve(vm)).0)
            .description(&self.description.resolve(vm))
            .with_size(parse_switch_size(&self.size.resolve(vm)));
        if let Some(align) = crate::common::content_alignment(&self.check_align.resolve(vm)) {
            sw.check_align = align;
        }
        sw
    }

    /// The canvas-independent half of a frame — see [`ButtonNode::interact`],
    /// whose shape this mirrors exactly, plus the toggle's write-back: on a
    /// completed click, the *new* checked state is written to `ix.vm` when
    /// the binding is `Mode=TwoWay` (§3), and `OnToggled` is dispatched
    /// regardless (§7's `Offline` switch: one-way, no write-back, but the
    /// handler still fires — see that worked example for why binding cannot
    /// be trusted to guess the side effect). `pub(crate)` for the same
    /// reason as `ButtonNode::interact`: `node::tests` exercises toggle →
    /// handler dispatch and the write-back without a live `Canvas`.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let on_value = self.on.resolve(ix.vm);
        let sw = self.build(on_value, ix.vm);
        let hot = !ix.frame.pointer_outside() && sw.hit_test(bounds, ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, toggled) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let toggled = toggled || ix.activate;
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        if toggled {
            let new_on = !on_value;
            if let Some(spec) = self.on.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(ix.vm, Value::Bool(new_on));
                }
            }
            let mut args = CheckedChangedEventArgs::new(on_value, new_on, ChangeSource::User);
            ix.fire("OnCheckedChanged", self.focus_id, self.on_toggled.as_deref(), ViewEventKind::Toggled(new_on), &mut args);
        }
        state
    }
}

impl ViewNode for SwitchNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(self.on.resolve(vm), vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        // Re-resolved AFTER `interact`, so a `Mode=TwoWay` write-back (or a
        // handler that itself calls `vm.set` on this same path — the fix for
        // the "handled but the switch stays put" case: `OnToggled` is the
        // one place a handler is trusted to own the value, per §3/§7) shows
        // up on THIS frame's paint, not one frame late.
        let raw = self.label.resolve(cx.vm);
        let _ = cx.mnemonic_text(&raw, true, crate::common::MnemonicAction::Activate);
        let on_value = self.on.resolve(cx.vm);
        let sw = self.build(on_value, cx.vm);
        sw.paint(cx.canvas, bounds, state);
    }
}

fn parse_switch_size(s: &str) -> SwitchSize {
    match s {
        "Sm" => SwitchSize::Sm,
        _ => SwitchSize::Md,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// TextField
// ─────────────────────────────────────────────────────────────────────────

pub struct TextFieldNode {
    pub text: PropSource<String>,
    pub placeholder: PropSource<String>,
    pub invalid: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    field: UiTextField,
    /// Whether [`Self::field`] has been seeded with an initial value yet —
    /// the reconciliation [`Self::paint`] does between the bound value and
    /// live edit state only overwrites the field's text before the field has
    /// ever been focused/edited once, so a keystroke is never clobbered by a
    /// binding re-read the same frame it fired.
    synced: bool,
    /// The `TextBoxBase` properties (`ReadOnly`, `MaxLength`, `PasswordChar`…), when written.
    text_box: Option<crate::common::TextBoxProps>,
}

impl TextFieldNode {
    pub fn new(
        text: PropSource<String>,
        placeholder: PropSource<String>,
        invalid: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
    ) -> Self {
        Self { text, placeholder, invalid, focus_id, on_changed, field: UiTextField::new(), synced: false, text_box: None }
    }

    /// Builder: the `TextBoxBase` properties.
    pub fn with_text_box(mut self, props: crate::common::TextBoxProps) -> Self {
        self.text_box = Some(props);
        self
    }
}

impl ViewNode for TextFieldNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.field.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let bound_text = self.text.resolve(cx.vm);
        if let Some(tb) = &self.text_box {
            tb.apply(&mut self.field, &*cx.vm);
        }
        let opts = self.text_box.as_ref().map(|tb| tb.focus_opts(false)).unwrap_or(FocusOpts::TEXT);
        let focus_state = self.focus_id.map(|id| cx.focus.register_with(id, bounds, opts)).unwrap_or_default();

        // Only overwrite the live edit buffer from the binding before the
        // field has taken its first edit: afterwards the field's own buffer
        // is the source of truth for what the user is typing, and the
        // two-way write-back below is what keeps `bound_text` caught up.
        // Always the model's REAL text, never `display()`: a password field displays its glyphs,
        // and comparing or writing those back would hand the view model « •••• » for the password.
        if !self.synced || (!focus_state.focused && self.field.text() != bound_text) {
            self.field.reset_text(&bound_text);
            self.synced = true;
        }
        self.field.placeholder_text = self.placeholder.resolve(cx.vm);
        self.field.invalid = self.invalid.resolve(cx.vm);

        let canvas: &dyn Canvas = cx.canvas;
        let input = EditInput::new(cx.frame, focus_state);
        let outcome = self.field.update(canvas, bounds, &input);
        let state = focus_state.apply(crate::common::rest());
        self.field.paint(canvas, bounds, state);
        // Enter in a single-line field: the view's AcceptButton gets it (WinForms).
        if outcome.submitted {
            crate::common::submit(cx.services.as_deref_mut());
        }

        if outcome.changed {
            let new_text = self.field.text().to_string();
            if let Some(spec) = self.text.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(new_text.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(bound_text, new_text.clone(), ChangeSource::User);
            cx.fire("OnTextChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new_text), &mut args);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Card — a single-child surface, painted through the real `Card::paint_body`
// so the header/footer/frame chrome stays byte-for-byte what a hand-written
// caller gets; only the body's child is routed through this crate's own
// recursion (see the module doc).
// ─────────────────────────────────────────────────────────────────────────

pub struct CardNode {
    pub title: PropSource<String>,
    pub subtitle: PropSource<String>,
    pub dense: PropSource<bool>,
    pub flush: PropSource<bool>,
    pub surface: PropSource<String>,
    pub child: Option<Box<dyn ViewNode>>,
}

impl CardNode {
    fn build(&self, vm: &dyn ViewModel) -> Card {
        let mut card = Card::new();
        card.set_title(self.title.resolve(vm));
        let mut card = card.with_subtitle(self.subtitle.resolve(vm));
        if self.dense.resolve(vm) {
            card = card.dense();
        }
        if self.flush.resolve(vm) {
            card = card.flush();
        }
        match self.surface.resolve(vm).as_str() {
            "Layer" => card.on_layer(),
            "Raised" => card.raised(),
            _ => card,
        }
    }
}

impl ViewNode for CardNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        // The card's own chrome, widened to the header's intrinsic width;
        // the body's height is added on top, since `Card::measure` (built
        // for the case where children are its own `Panel` slots) knows
        // nothing of a child this crate lays out itself.
        let base = self.build(vm).measure(c);
        let child = self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::EMPTY);
        Size::new(base.width.max(child.width), base.height + child.height)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let card = self.build(cx.vm);
        let canvas: &dyn Canvas = cx.canvas;
        let child = &mut self.child;
        let mut inner = cx.reborrow();
        card.paint_body(canvas, bounds, crate::common::rest(), move |_c, body_rect| {
            if let Some(node) = child.as_mut() {
                node.paint(&mut inner, body_rect);
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Stack — every child laid out by the real `flow_layout` (via
// `kubuno_ui::containers::Stack::layout_children`), painted by this crate.
// ─────────────────────────────────────────────────────────────────────────

/// One `<Stack>` child, plus an optional EXPLICIT main-axis extent read off
/// its own `Height`/`Width` XML attribute at build time (`COMMON_ATTRIBUTES`
/// already validates these as allowed on any element; a `<Stack>` child is
/// now the second place, after `<Panel>`'s Dock/Anchor children, that
/// actually reads them). `None` keeps the block sized from the child's own
/// [`ViewNode::measure`]/[`ViewNode::measure_for_width`] — right for the
/// overwhelming majority of children; `Some(extent)` is the escape hatch for
/// a widget whose own measurement is a designer placeholder unrelated to how
/// tall/wide it is actually meant to be in this view (`<Splitter>`, whose
/// two children can be any size at all, is the worked example).
pub struct StackChild {
    pub node: Box<dyn ViewNode>,
    /// Overrides the block's height in a vertical (column) stack.
    pub explicit_height: Option<f32>,
    /// Overrides the block's width in a horizontal (row) stack.
    pub explicit_width: Option<f32>,
    /// `Stack.Fill="true"`: the child takes the room the others leave along the flow.
    pub fill: bool,
}

impl StackChild {
    /// A child with no explicit extent — measured normally.
    pub fn measured(node: Box<dyn ViewNode>) -> Self {
        Self { node, explicit_height: None, explicit_width: None, fill: false }
    }
}

pub struct StackNode {
    pub direction: PropSource<String>,
    pub gap: PropSource<f32>,
    pub padding: PropSource<f32>,
    pub surface: PropSource<String>,
    /// `WrapContents`: the children wrap onto several lines (WinForms `FlowLayoutPanel`).
    pub wrap: PropSource<bool>,
    /// `CrossAlign`: `Stretch` (the default: a column's children are its width), `Start`,
    /// `Center` or `End`.
    pub align: PropSource<String>,
    pub children: Vec<StackChild>,
}

impl StackNode {
    /// Unconstrained — what [`ViewNode::measure`] (a parent asking "how big
    /// do you want to be", with no known final width yet) uses. Each
    /// vertical child's own extent comes from its plain, unconstrained
    /// [`ViewNode::measure`], same as before this node gained
    /// [`Self::build_for_bounds`].
    fn build(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Stack {
        self.build_inner(c, vm, None)
    }

    /// What [`ViewNode::paint`] uses, now that the real `bounds` — and so
    /// the real row width a vertical child is about to be painted into — is
    /// known: each vertical child is measured with
    /// [`ViewNode::measure_for_width`] against that width instead of its own
    /// unconstrained guess. See that method's doc for why this matters (a
    /// `<Callout>` measured against the wrong width reports the wrong
    /// height, sometimes wildly so — this is the fix).
    fn build_for_bounds(&self, c: &dyn Canvas, vm: &dyn ViewModel, bounds: Rect) -> Stack {
        self.build_inner(c, vm, Some(bounds))
    }

    fn build_inner(&self, c: &dyn Canvas, vm: &dyn ViewModel, bounds: Option<Rect>) -> Stack {
        let direction = parse_flow_direction(&self.direction.resolve(vm));
        let mut stack = Stack::column(self.gap.resolve(vm));
        stack.direction = direction;
        let mut stack = stack.with_padding(Padding::all(self.padding.resolve(vm))).with_surface(parse_surface(&self.surface.resolve(vm)));
        let horizontal = matches!(direction, FlowDirection::LeftToRight | FlowDirection::RightToLeft);
        // A column's blocks are always the FULL row wide (`Stack::column`'s
        // own design — see `ViewNode::measure_for_width`'s doc); that width
        // is `display_rect`'s, independent of the blocks' own extents, so it
        // is known before the loop below computes them.
        let row_width = if horizontal {
            None
        } else {
            bounds.map(|b| {
                let d = stack.display_rect(b);
                d.right - d.left
            })
        };
        for child in &self.children {
            let explicit = if horizontal { child.explicit_width } else { child.explicit_height };
            let extent = match explicit {
                Some(e) => e,
                None => {
                    let size = match row_width {
                        Some(w) => child.node.measure_for_width(c, vm, w),
                        None => child.node.measure(c, vm),
                    };
                    if horizontal {
                        size.width
                    } else {
                        size.height
                    }
                }
            };
            stack = stack.block(extent.max(0.0));
        }
        stack
    }
}

impl StackNode {
    /// A child's size along and across the flow (explicit extents first).
    fn child_size(child: &StackChild, c: &dyn Canvas, vm: &dyn ViewModel, horizontal: bool, cross_avail: f32) -> (f32, f32) {
        let size = if horizontal { child.node.measure(c, vm) } else { child.node.measure_for_width(c, vm, cross_avail) };
        let (w, h) = (child.explicit_width.unwrap_or(size.width), child.explicit_height.unwrap_or(size.height));
        let w = if horizontal { w } else { child.explicit_width.or_else(|| child.node.intrinsic_width(c, vm)).unwrap_or(w) };
        if horizontal { (w.max(0.0), h.max(0.0)) } else { (h.max(0.0), w.max(0.0)) }
    }

    /// The children's rectangles in `area` for `WrapContents`, `CrossAlign` and `Stack.Fill`.
    #[allow(clippy::too_many_arguments)]
    fn flex_layout(&self, c: &dyn Canvas, vm: &dyn ViewModel, area: Rect, direction: FlowDirection, gap: f32, wrap: bool, align: &str) -> Vec<Rect> {
        let horizontal = matches!(direction, FlowDirection::LeftToRight | FlowDirection::RightToLeft);
        let reversed = matches!(direction, FlowDirection::RightToLeft | FlowDirection::BottomUp);
        let (main_len, cross_len) = if horizontal { (area.right - area.left, area.bottom - area.top) } else { (area.bottom - area.top, area.right - area.left) };
        let sizes: Vec<(f32, f32)> = self.children.iter().map(|ch| Self::child_size(ch, c, vm, horizontal, cross_len)).collect();
        // Lines of children: (first index, end index, cross extent of the line).
        let mut lines: Vec<(usize, usize, f32)> = Vec::new();
        if wrap {
            let (mut start, mut used, mut cross) = (0usize, 0.0f32, 0.0f32);
            for (i, (m, x)) in sizes.iter().enumerate() {
                let need = if i > start { used + gap + m } else { *m };
                if i > start && need > main_len {
                    lines.push((start, i, cross));
                    start = i;
                    used = *m;
                    cross = *x;
                } else {
                    used = need;
                    cross = cross.max(*x);
                }
            }
            if start < sizes.len() {
                lines.push((start, sizes.len(), cross));
            }
        } else {
            lines.push((0, sizes.len(), cross_len));
        }
        let mut out = vec![Rect::default(); sizes.len()];
        let mut cross_at = 0.0f32;
        for (start, end, line_cross) in lines {
            // Along the flow: the fill children share what the others leave.
            let fills = (start..end).filter(|&i| self.children[i].fill).count();
            let fixed: f32 = (start..end).filter(|&i| !self.children[i].fill).map(|i| sizes[i].0).sum::<f32>() + gap * (end - start).saturating_sub(1) as f32;
            let share = if fills > 0 { ((main_len - fixed) / fills as f32).max(0.0) } else { 0.0 };
            let mut main_at = 0.0f32;
            for i in start..end {
                let m = if self.children[i].fill { share } else { sizes[i].0 };
                let x = sizes[i].1.min(line_cross);
                let (cross_off, cross_size) = match align {
                    "Start" => (0.0, x),
                    "Center" => ((line_cross - x) / 2.0, x),
                    "End" => (line_cross - x, x),
                    _ => (0.0, line_cross),
                };
                let lead = if reversed { main_len - main_at - m } else { main_at };
                out[i] = if horizontal {
                    let l = area.left + lead;
                    let t = area.top + cross_at + cross_off;
                    Rect::new(l, t, l + m, t + cross_size)
                } else {
                    let t = area.top + lead;
                    let l = area.left + cross_at + cross_off;
                    Rect::new(l, t, l + cross_size, t + m)
                };
                main_at += m + gap;
            }
            cross_at += line_cross + gap;
        }
        out
    }
}

impl ViewNode for StackNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let size = self.build(c, vm).measure(c);
        // A row's blocks carry only their widths: its height is its tallest child's, so a child that
        // grows (an auto-growing `TextArea`, `MinLines`/`MaxLines`) grows a row a layout sizes
        // (`AutoSize`, a `Dock="Bottom"` band).
        let horizontal = matches!(parse_flow_direction(&self.direction.resolve(vm)), FlowDirection::LeftToRight | FlowDirection::RightToLeft);
        if horizontal && !self.wrap.resolve(vm) && !self.children.is_empty() {
            let cross = self.children.iter().map(|ch| Self::child_size(ch, c, vm, true, 0.0).1).fold(0.0f32, f32::max);
            return Size::new(size.width, size.height.max(cross + 2.0 * self.padding.resolve(vm)));
        }
        size
    }

    fn is_invisible_container(&self, vm: &dyn ViewModel) -> bool {
        matches!(parse_surface(&self.surface.resolve(vm)), Surface::None)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        let stack = self.build_for_bounds(canvas, cx.vm, bounds);
        stack.paint(canvas, bounds, crate::common::rest());
        let horizontal = matches!(stack.direction, FlowDirection::LeftToRight | FlowDirection::RightToLeft);
        let wrap = self.wrap.resolve(cx.vm);
        let align = self.align.resolve(cx.vm);
        if wrap || align != "Stretch" || self.children.iter().any(|c| c.fill) {
            // Wrapping, cross alignment or a filling child: laid out here (the `kubuno_ui` stack
            // has none of the three), inside the same padded display rectangle.
            // `display_rect` is in the stack's LOCAL space: moved to the canvas by its content origin.
            let local = stack.display_rect(bounds);
            let origin = stack.content_origin(bounds);
            let area = Rect::new(local.left + origin.x, local.top + origin.y, local.right + origin.x, local.bottom + origin.y);
            let rects = self.flex_layout(canvas, cx.vm, area, stack.direction, stack.gap, wrap, &align);
            for (child, rect) in self.children.iter_mut().zip(rects) {
                let mut inner = cx.reborrow();
                child.node.paint(&mut inner, rect);
            }
            return;
        }
        let rects = stack.layout_children(bounds);
        for (child, rect) in self.children.iter_mut().zip(rects) {
            // A vertical column's block is always full-row-wide (see
            // `build_inner`'s own note); narrow it to the child's own
            // `intrinsic_width`, left-aligned, when it declares one — see
            // `ViewNode::intrinsic_width`'s doc for which widgets need this
            // (a fixed-content-size one like `Badge`/`IconButton`) and which
            // must NOT get it (a `w-full` one like `Separator`/`ProgressBar`,
            // whose default `None` answer keeps today's full-width stretch).
            let rect = if horizontal {
                rect
            } else if child.explicit_width.is_some() {
                // An explicit `Width` already says exactly how wide this
                // child wants to be, the same way `explicit_height`/
                // `explicit_width` already overrode its main-axis extent
                // above — `intrinsic_width` would only be a second, weaker
                // opinion.
                rect
            } else {
                match child.node.intrinsic_width(canvas, cx.vm) {
                    Some(w) if w > 0.0 && w < (rect.right - rect.left) => Rect::new(rect.left, rect.top, rect.left + w, rect.bottom),
                    _ => rect,
                }
            };
            let mut inner = cx.reborrow();
            child.node.paint(&mut inner, rect);
        }
    }
}

fn parse_flow_direction(s: &str) -> FlowDirection {
    match s {
        "LeftToRight" => FlowDirection::LeftToRight,
        "RightToLeft" => FlowDirection::RightToLeft,
        "BottomUp" => FlowDirection::BottomUp,
        _ => FlowDirection::TopDown,
    }
}

fn parse_surface(s: &str) -> Surface {
    match s {
        "Layer" => Surface::Layer,
        "Card" => Surface::Card,
        "Raised" => Surface::Raised,
        "Well" => Surface::Well,
        _ => Surface::None,
    }
}

#[cfg(test)]
mod tests {
    //! Exercises [`ButtonNode::interact`]/[`SwitchNode::interact`] directly —
    //! the canvas-independent half of a frame [`PaintCx::interact_cx`] splits
    //! out — so a click/toggle's effect on the view model and the event/
    //! handler dispatch can be checked without a live `kubuno_ui::Canvas`
    //! (see this crate's other tests, and `compile::tests`' own note, for why
    //! that matters: nothing in `kubuno-ui`'s own test suite constructs one
    //! either).
    //!
    //! `switch_toggle_one_way_binding_needs_the_handler_to_write_back` is the
    //! regression test for the bug the live preview caught: a `Mode=OneWay`
    //! `On` binding with an `OnToggled` handler stayed visually unchanged
    //! after a click, because nothing ever wrote the new value back to the
    //! path the binding reads. `SwitchNode::interact` was already correct —
    //! it always resolves `on_value` fresh — the missing piece was that nei
    //! ther the write-back (deliberately skipped for `OneWay`, per §3) nor
    //! the demo's own handler (`examples/view_preview.rs`'s `offline_toggled`,
    //! fixed alongside this test) updated the view model. This test pins the
    //! CONTRACT: an `OnToggled` handler that itself calls `ViewModel::set` on
    //! the bound path is enough to make the displayed value follow the click.

    use super::*;
    use crate::binding::{BindingSpec, MapViewModel};
    use crate::handlers;

    fn frame_at(mouse: (f32, f32), mouse_down: bool) -> Frame {
        Frame {
            size: (400.0, 300.0),
            mouse,
            mouse_down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 400.0, 300.0),
            chrome_top: 0.0,
            mods: kubuno_controls::host::Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        }
    }

    /// The four pieces of an [`InteractCx`] a test owns across several
    /// frames, bundled so a helper that drives one frame does not itself
    /// need clippy's `too_many_arguments` allowance.
    struct Harness {
        vm: MapViewModel,
        focus: FocusRing,
        handlers: HandlerTable,
        events: Vec<ViewEvent>,
    }

    impl Harness {
        fn new(vm: MapViewModel, handlers: HandlerTable) -> Self {
            Self { vm, focus: FocusRing::new(), handlers, events: Vec::new() }
        }

        /// Runs one `interact` frame at `mouse`/`mouse_down` over `bounds`,
        /// calling `act` (typically `|ix, b| node.interact(ix, b)`) with the
        /// resulting [`InteractCx`].
        fn frame(
            &mut self,
            bounds: Rect,
            mouse: (f32, f32),
            mouse_down: bool,
            mut act: impl FnMut(&mut InteractCx<'_>, Rect) -> WidgetState,
        ) -> WidgetState {
            let frame = frame_at(mouse, mouse_down);
            let mut ix = InteractCx::new(&frame, &mut self.vm, &mut self.focus, &mut self.handlers, &mut self.events);
            act(&mut ix, bounds)
        }
    }

    #[test]
    fn button_click_dispatches_its_handler_and_fires_an_event() {
        let mut node = ButtonNode::new(
            PropSource::Literal("Save".to_string()),
            PropSource::Literal("Primary".to_string()),
            PropSource::Literal("Md".to_string()),
            PropSource::Literal(String::new()),
            PropSource::Literal(false),
            None,
            Some("save_clicked".to_string()),
        );
        let handlers = handlers! {
            "save_clicked" => |vm, _v| { vm.set("Saved", Value::Bool(true)); },
        };
        let mut h = Harness::new(MapViewModel::new(), handlers);
        let bounds = Rect::new(0.0, 0.0, 80.0, 32.0);

        h.frame(bounds, (10.0, 10.0), true, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("Saved"), None, "a press alone is not yet a click");
        h.frame(bounds, (10.0, 10.0), false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("Saved"), Some(Value::Bool(true)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Clicked));
        assert_eq!(h.events[0].handler.as_deref(), Some("save_clicked"));
    }

    #[test]
    fn a_real_button_click_sits_between_the_routed_mouse_events() {
        use crate::ast::{AstNode, Document};
        use crate::events::router::{Dispatch, FrameInput, InputRouter, SlotEvents};

        let parse = crate::syntax::parse(
            r#"<Button x:Name="ok" OnClick="ok_click" OnMouseDown="ok_down" OnMouseClick="ok_mouse_click" OnMouseUp="ok_up"/>"#,
        );
        let element = Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap();
        let slot = Rc::new(SlotEvents::from_element(&element, crate::registry::lookup("Button").unwrap(), false));
        let mut node = ButtonNode::new(
            PropSource::Literal("Ok".to_string()),
            PropSource::Literal("Primary".to_string()),
            PropSource::Literal("Md".to_string()),
            PropSource::Literal(String::new()),
            PropSource::Literal(false),
            slot.focus_id,
            Some("ok_click".to_string()),
        );
        let clicks = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = clicks.clone();
        let mut handlers = HandlerTable::new();
        handlers.insert("ok_click", Box::new(move |_vm, v| seen.borrow_mut().push(v)));
        let (mut vm, mut focus, mut router) = (MapViewModel::new(), FocusRing::new(), InputRouter::new());
        let bounds = Rect::new(0.0, 0.0, 80.0, 32.0);
        let mut log = Vec::new();
        for (mouse, down) in [((10.0, 10.0), false), ((10.0, 10.0), false), ((10.0, 10.0), true), ((10.0, 10.0), false)] {
            let mut frame = frame_at(mouse, down);
            frame.click_count = 1;
            let mut events = Vec::new();
            focus.begin_frame(&frame);
            let mut d = Dispatch { vm: &mut vm, handlers: &mut handlers, events: &mut events };
            router.begin_frame(&FrameInput { frame: &frame, now_ms: 0, events: &[] }, &mut focus, &mut d);
            router.register(slot.clone(), bounds);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            ix.sender = Some((slot.clone(), bounds));
            node.interact(&mut ix, bounds);
            let mut d = Dispatch { vm: &mut vm, handlers: &mut handlers, events: &mut events };
            router.end_frame(&mut d);
            focus.end_frame();
            log.extend(events.into_iter().filter_map(|e| e.handler));
        }
        assert_eq!(log, ["ok_down", "ok_click", "ok_mouse_click", "ok_up"]);
        assert_eq!(*clicks.borrow(), vec![Value::Bool(true)], "the legacy handler still gets Bool(true)");
    }

    /// EVT-4: the same real button click, dispatched to typed `#[event_handlers]` methods of
    /// the view model (through `TypedViewModel`, what `Runtime::frame_typed` paints with), with
    /// a legacy table entry for the one event the sink does not handle.
    #[test]
    fn a_real_button_click_reaches_typed_handlers_with_a_typed_sender_and_mouse_args() {
        use crate::ast::{AstNode, Document};
        use crate::controls::Button as ButtonControl;
        use crate::events::router::{Dispatch, FrameInput, InputRouter, SlotEvents};
        use crate::events::{MouseButton, MouseEventArgs, Sender, TypedViewModel};

        #[derive(Default)]
        struct Vm {
            log: Vec<String>,
        }
        impl ViewModel for Vm {
            fn get(&self, _path: &str) -> Option<Value> {
                None
            }
            fn set(&mut self, path: &str, value: Value) {
                self.log.push(format!("set {path} {value:?}"));
            }
        }
        #[crate::event_handlers]
        impl Vm {
            fn ok_click(&mut self, sender: &Sender<ButtonControl>, e: &MouseEventArgs) {
                self.log.push(format!("click {} {:?} {} at {},{}", sender.text(), e.button, e.clicks, e.x, e.y));
            }
            fn ok_down(&mut self, sender: &Sender<ButtonControl>, e: &mut MouseEventArgs) {
                self.log.push(format!("down {} {:?}", sender.name().unwrap_or(""), e.button));
            }
            fn ok_mouse_click(&mut self) {
                self.log.push("mouse click".into());
            }
        }

        let parse = crate::syntax::parse(
            r#"<Button x:Name="ok" Text="Ok" OnClick="ok_click" OnMouseDown="ok_down" OnMouseClick="ok_mouse_click" OnMouseUp="ok_up"/>"#,
        );
        let element = Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap();
        let slot = Rc::new(SlotEvents::from_element(&element, crate::registry::lookup("Button").unwrap(), false));
        let mut node = ButtonNode::new(
            PropSource::Literal("Ok".to_string()),
            PropSource::Literal("Primary".to_string()),
            PropSource::Literal("Md".to_string()),
            PropSource::Literal(String::new()),
            PropSource::Literal(false),
            slot.focus_id,
            Some("ok_click".to_string()),
        );
        let mut handlers = handlers! { "ok_up" => |vm, value| vm.set("Up", value) };
        let mut model = Vm::default();
        let (mut focus, mut router) = (FocusRing::new(), InputRouter::new());
        let bounds = Rect::new(10.0, 20.0, 90.0, 52.0);
        for (mouse, down) in [((15.0, 25.0), false), ((15.0, 25.0), false), ((15.0, 25.0), true), ((16.0, 26.0), false)] {
            let mut vm = TypedViewModel(&mut model);
            let mut frame = frame_at(mouse, down);
            frame.click_count = 1;
            let mut events = Vec::new();
            focus.begin_frame(&frame);
            let mut d = Dispatch { vm: &mut vm, handlers: &mut handlers, events: &mut events };
            router.begin_frame(&FrameInput { frame: &frame, now_ms: 0, events: &[] }, &mut focus, &mut d);
            router.register(slot.clone(), bounds);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            ix.sender = Some((slot.clone(), bounds));
            node.interact(&mut ix, bounds);
            let mut d = Dispatch { vm: &mut vm, handlers: &mut handlers, events: &mut events };
            router.end_frame(&mut d);
            focus.end_frame();
        }
        assert_eq!(
            model.log,
            ["down ok Left", "click Ok Left 1 at 6,6", "mouse click", "set Up Bool(true)"],
            "typed handlers in the WinForms order; the legacy entry gets its legacy value"
        );
        assert_eq!(MouseButton::default(), MouseButton::None);
    }

    #[test]
    fn button_click_outside_after_press_is_not_a_click() {
        let mut node = ButtonNode::new(
            PropSource::Literal("Save".to_string()),
            PropSource::Literal("Primary".to_string()),
            PropSource::Literal("Md".to_string()),
            PropSource::Literal(String::new()),
            PropSource::Literal(false),
            None,
            Some("save_clicked".to_string()),
        );
        let mut h = Harness::new(MapViewModel::new(), HandlerTable::new());
        let bounds = Rect::new(0.0, 0.0, 80.0, 32.0);

        h.frame(bounds, (10.0, 10.0), true, |ix, b| node.interact(ix, b));
        // Drag off the button, then release outside it.
        h.frame(bounds, (500.0, 500.0), false, |ix, b| node.interact(ix, b));
        assert!(h.events.is_empty());
    }

    #[test]
    fn switch_toggle_two_way_binding_writes_back_and_fires_an_event() {
        let mut node = SwitchNode::new(
            PropSource::Bound { spec: BindingSpec { path: "On".to_string(), mode: BindingMode::TwoWay, ..Default::default() }, fallback: false },
            PropSource::Literal(String::new()),
            PropSource::Literal(String::new()),
            PropSource::Literal("Md".to_string()),
            None,
            None,
        );
        let mut h = Harness::new(MapViewModel::new().with("On", Value::Bool(false)), HandlerTable::new());
        let bounds = Rect::new(0.0, 0.0, 40.0, 24.0);

        h.frame(bounds, (10.0, 10.0), true, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("On"), Some(Value::Bool(false)), "not toggled on press alone");
        h.frame(bounds, (10.0, 10.0), false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("On"), Some(Value::Bool(true)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Toggled(true)));
        // What the widget would be rebuilt with on the very next resolve —
        // proof the DISPLAY would follow, not just the raw view-model value.
        assert!(node.on.resolve(&h.vm));
    }

    #[test]
    fn switch_toggle_one_way_binding_needs_the_handler_to_write_back() {
        let mut node = SwitchNode::new(
            PropSource::Bound { spec: BindingSpec { path: "Offline".to_string(), mode: BindingMode::OneWay, ..Default::default() }, fallback: false },
            PropSource::Literal(String::new()),
            PropSource::Literal(String::new()),
            PropSource::Literal("Md".to_string()),
            None,
            Some("offline_toggled".to_string()),
        );
        let handlers = handlers! {
            "offline_toggled" => |vm, v| {
                if let Value::Bool(on) = v {
                    vm.set("Offline", Value::Bool(on));
                }
            },
        };
        let mut h = Harness::new(MapViewModel::new().with("Offline", Value::Bool(false)), handlers);
        let bounds = Rect::new(0.0, 0.0, 40.0, 24.0);

        h.frame(bounds, (10.0, 10.0), true, |ix, b| node.interact(ix, b));
        h.frame(bounds, (10.0, 10.0), false, |ix, b| node.interact(ix, b));

        // The interpreter's OWN two-way write-back is skipped (`Mode=OneWay`)
        // — this is the handler's write taking effect, exactly §7's "binding
        // cannot guess [the side effect]; a handler can".
        assert_eq!(h.vm.get("Offline"), Some(Value::Bool(true)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Toggled(true)));
        assert!(node.on.resolve(&h.vm), "the displayed value must follow the handler's write");
    }

    #[test]
    fn switch_with_no_handler_and_a_one_way_binding_does_not_silently_write_back() {
        // The interpreter itself must never promote a `Mode=OneWay` binding
        // to a write — that is exactly what `Mode=TwoWay` is for (§3). A
        // one-way switch with NO handler is legitimately "display only, the
        // click is dropped visually" (the design note's own escape hatch:
        // the caller is expected to give it a handler, or `Mode=TwoWay`).
        let mut node = SwitchNode::new(
            PropSource::Bound { spec: BindingSpec { path: "Offline".to_string(), mode: BindingMode::OneWay, ..Default::default() }, fallback: false },
            PropSource::Literal(String::new()),
            PropSource::Literal(String::new()),
            PropSource::Literal("Md".to_string()),
            None,
            None,
        );
        let mut h = Harness::new(MapViewModel::new().with("Offline", Value::Bool(false)), HandlerTable::new());
        let bounds = Rect::new(0.0, 0.0, 40.0, 24.0);

        h.frame(bounds, (10.0, 10.0), true, |ix, b| node.interact(ix, b));
        h.frame(bounds, (10.0, 10.0), false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("Offline"), Some(Value::Bool(false)));
        // The click still happened and is still reported — a caller with no
        // handler registered can still see it in the returned events.
        assert_eq!(h.events.len(), 1);
    }

    #[test]
    fn a_value_the_handler_wrote_survives_a_hot_reload() {
        // `XML_VIEWS.md` §5: "reloading is naturally non-destructive: only
        // the recipe … is replaced". The view model lives OUTSIDE the
        // compiled tree, so a value a handler wrote before a reload is still
        // there for a brand-new node — built with none of the old node's
        // `pressed`/whatever internal state — to read on its very first
        // resolve, with no replayed interaction at all.
        let vm = MapViewModel::new().with("Offline", Value::Bool(true));
        let fresh_after_reload = SwitchNode::new(
            PropSource::Bound { spec: BindingSpec { path: "Offline".to_string(), mode: BindingMode::OneWay, ..Default::default() }, fallback: false },
            PropSource::Literal(String::new()),
            PropSource::Literal(String::new()),
            PropSource::Literal("Md".to_string()),
            None,
            Some("offline_toggled".to_string()),
        );
        assert!(
            fresh_after_reload.on.resolve(&vm),
            "a reloaded tree must read the surviving view-model value, not the XML literal's fallback"
        );
    }
}

thread_local! {
    /// How many events answered the pointer so far (a click, a toggle, a pick): a list whose items
    /// hold controls compares it around an item to know whether a control inside handled the click
    /// (WPF's `e.Handled`), so the item itself does not also report it.
    static POINTER_HANDLED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Whether an event of this kind answers the pointer (see [`pointer_handled`]).
fn takes_pointer(kind: &ViewEventKind) -> bool {
    match kind {
        ViewEventKind::Clicked | ViewEventKind::Toggled(_) | ViewEventKind::Changed(_) => true,
        ViewEventKind::Other { name, .. } => name.contains("Click") || *name == "MouseDown" || *name == "MouseUp",
    }
}

/// The count of events that answered the pointer on this thread so far: compare it before and
/// after painting a part of the view to know whether a control in it handled the pointer.
pub fn pointer_handled() -> u64 {
    POINTER_HANDLED.with(|c| c.get())
}

/// A floating part of the view (`<Popover>`): painted above the whole view, after it, whatever its
/// place in the document, and able to keep the pointer from the view under it while it is open.
pub(crate) trait TopLayer {
    /// Paints it above the view (`window`: the whole view's box). Its `cx.frame` is the real one,
    /// even while the view under it sees the pointer away.
    fn paint_top(&mut self, cx: &mut PaintCx<'_>, window: Rect);
    /// What it keeps from the view under it next frame: `None`, nothing; `Some(None)`, every
    /// pointer event (a light-dismiss panel: the press outside it only closes it); `Some(Some(r))`,
    /// the pointer over `r`.
    fn hold(&self) -> Option<Option<Rect>>;
}

thread_local! {
    /// What the floating parts of the view keep from it (see [`TopLayer::hold`]), from the frame
    /// before: the runtime shows the view the pointer away accordingly.
    static TOP_LAYER_HOLD: std::cell::RefCell<Vec<Option<Rect>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The frame as it came, while the runtime shows the view a masked one.
    static REAL_FRAME: std::cell::Cell<Option<Frame>> = const { std::cell::Cell::new(None) };
}

/// Whether a floating part of the view keeps the pointer at `(x, y)` from the view under it.
pub(crate) fn top_layer_holds(x: f32, y: f32) -> bool {
    TOP_LAYER_HOLD.with(|h| h.borrow().iter().any(|r| r.is_none_or(|r| r.contains(x, y))))
}

/// Sets (or clears) the real frame of this paint, which the floating parts read.
pub(crate) fn set_real_frame(frame: Option<Frame>) {
    REAL_FRAME.with(|f| f.set(frame));
}

/// The root of a view with floating parts: the view, then its [`TopLayer`]s above it.
pub(crate) struct TopLayerRoot {
    pub(crate) root: Box<dyn ViewNode>,
    pub(crate) layer: Vec<std::rc::Rc<std::cell::RefCell<dyn TopLayer>>>,
}

impl ViewNode for TopLayerRoot {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.root.measure(c, vm)
    }

    fn measure_for_width(&self, c: &dyn Canvas, vm: &dyn ViewModel, width: f32) -> Size {
        self.root.measure_for_width(c, vm, width)
    }

    fn intrinsic_width(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Option<f32> {
        self.root.intrinsic_width(c, vm)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.root.paint(cx, bounds);
        if cx.design.is_some() {
            // In the designer each panel is edited in place (see `<Popover>`).
            return;
        }
        let real = REAL_FRAME.with(|f| f.get());
        let mut holds = Vec::new();
        // Above the whole view: out of the clips of the containers they are declared in (a popover declared in a
        // user control's view is not cut to the user control).
        crate::clip::detached(|| {
            for l in &self.layer {
                let Ok(mut l) = l.try_borrow_mut() else { continue };
                match real {
                    Some(f) => {
                        let mut inner = cx.reborrow();
                        inner.frame = &f;
                        l.paint_top(&mut inner, bounds);
                    }
                    None => l.paint_top(cx, bounds),
                }
                if let Some(h) = l.hold() {
                    holds.push(h);
                }
            }
        });
        TOP_LAYER_HOLD.with(|h| *h.borrow_mut() = holds);
    }
}
