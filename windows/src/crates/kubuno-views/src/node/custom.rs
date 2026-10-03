//! The view nodes of application classes (EVT-7b of `vskubuno/docs/EVENTS.md`) and of the
//! built-in non-visual components:
//!
//! - [`CustomControlNode`]: a control of the application written from a level
//!   (`#[kubuno(extends = Control)]`): painted by its class's `on_paint_background` / `on_paint`,
//!   sized by its `get_preferred_size`. (A class extending a built-in class — `extends = Button`
//!   — reuses that class's node: `<Button>`'s node paints through the class's `on_paint`.)
//! - [`UserControlNode`]: a user control, `<RatingBar/>`: its own `.kbview` (embedded by
//!   `#[derive(UserControl)]`), compiled once and painted inside the element with the user
//!   control's instance as the view model — its bindings read its properties, its handlers are
//!   its methods.
//! - [`NonVisualNode`] / [`TimerNode`]: components that paint nothing (the designer lists them in
//!   its component tray); a `<Timer>` raises `Tick` every `Interval` milliseconds while `Enabled`.
//! - [`PlaceholderNode`]: a class the tools only know from its source (not compiled into this
//!   program, e.g. the design surface before the project was built): a labelled dashed box.
//!
//! The element's class instance is owned by its `crate::design::DesignSlot`, which lends it to
//! the node (`PaintCx::control`) and applies the class's own properties from the element's
//! attributes before the node paints.

use std::cell::Cell;
use std::time::{Duration, Instant};

use kubuno_controls::host::{self, Frame};
use kubuno_ui::{Canvas, FocusId, Rect, Size, WidgetState};

use super::{press_release, PaintCx, ViewEvent, ViewNode};
use crate::binding::{HandlerTable, PropSource, ViewModel};
use crate::component::EventCx;
use crate::events::EmptyEventArgs;
use crate::node::ViewEventKind;
use crate::props::{BuildCx, BuildError, Props};
use crate::registry::project;

fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

// ── Placeholder ─────────────────────────────────────────────────────────────────────────────

/// A class known from its source only (see the module doc): a dashed box with its name.
pub struct PlaceholderNode {
    name: &'static str,
}

/// The `build` of a declared class.
pub fn placeholder_build(props: &Props<'_>, _cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    Ok(Box::new(PlaceholderNode { name: props.meta().name }))
}

/// Paints the placeholder of `name` in `bounds`: a dashed outline, the name, and what to do.
pub fn paint_placeholder(c: &dyn kubuno_controls::ControlCanvas, bounds: Rect, name: &str, hint: &str) {
    if bounds.right - bounds.left < 2.0 || bounds.bottom - bounds.top < 2.0 {
        return;
    }
    let theme = c.theme();
    c.fill_rect(&bounds, &theme.window_background);
    for dash in crate::design::dashed_outline(bounds, 4.0, 3.0, 1.0) {
        c.fill_rect(&dash, &theme.text_secondary);
    }
    let inner = Rect::new(bounds.left + 6.0, bounds.top + 2.0, bounds.right - 6.0, bounds.bottom - 2.0);
    // The text is centred vertically in its rectangle: the name alone, or the name over the hint.
    if inner.bottom - inner.top >= 36.0 {
        let mid = (inner.top + inner.bottom) / 2.0;
        c.text_ellipsis(name, &Rect::new(inner.left, inner.top, inner.right, mid), &c.formats().body, &theme.text_primary);
        c.text_ellipsis(hint, &Rect::new(inner.left, mid, inner.right, inner.bottom), &c.formats().caption, &theme.text_secondary);
    } else {
        c.text_ellipsis(name, &inner, &c.formats().body, &theme.text_primary);
    }
}

impl ViewNode for PlaceholderNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size { width: 160.0, height: 40.0 }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        paint_placeholder(cx.canvas, bounds, self.name, &crate::messages::tr("Build the project to preview", "Générez le projet pour l'afficher"));
    }
}

// ── Custom control ──────────────────────────────────────────────────────────────────────────

/// A control of the application written from a level (see the module doc).
pub struct CustomControlNode {
    name: &'static str,
    focus_id: Option<FocusId>,
    /// The element's `OnPaint` handler (raised with the surface lent).
    on_paint: Option<String>,
    /// The element's `OnClick` handler: an accessibility client's Invoke (UI Automation, a screen
    /// reader) clicks the control through it.
    on_click: Option<String>,
    pressed: bool,
    /// The class's `get_preferred_size` at the last paint (a layout measures before it paints).
    preferred: Cell<Option<Size>>,
}

/// The `build` of a linked control class extending a level.
pub fn custom_control_build(props: &Props<'_>, _cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    Ok(Box::new(CustomControlNode {
        name: props.meta().name,
        focus_id: props.focus_id(),
        on_paint: props.event("OnPaint"),
        on_click: props.event("OnClick"),
        pressed: false,
        preferred: Cell::new(None),
    }))
}

impl ViewNode for CustomControlNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.preferred.get().filter(|s| s.width > 0.0 && s.height > 0.0).unwrap_or(Size { width: 100.0, height: 36.0 })
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        // Invoked by an accessibility client (the window's `AccessAction::Click`): a click of the
        // control, through its class (`on_click`) then the element's handler — as a real click would.
        if cx.activate && cx.control.as_deref().is_some_and(|c| c.enabled()) {
            let mut args = crate::events::MouseEventArgs { button: crate::events::MouseButton::Left, clicks: 1, ..Default::default() };
            cx.fire_quiet("OnClick", self.on_click.as_deref(), &mut args);
        }
        let frame: &Frame = cx.frame;
        let hot = !frame.pointer_outside() && contains(bounds, frame.mouse.0, frame.mouse.1);
        let (down, _clicked) = press_release(&mut self.pressed, hot, frame.mouse_down);
        let selectable = cx.control.as_deref().is_some_and(|c| c.can_select());
        let focus_state = match (self.focus_id, selectable) {
            (Some(id), true) => cx.focus.register(id, bounds),
            _ => Default::default(),
        };
        let enabled = cx.control.as_deref().is_none_or(|c| c.enabled()) && !crate::common::is_disabled();
        let state = focus_state.apply(WidgetState::REST.hot(hot && enabled).pressed(down && enabled)).disabled(!enabled);
        let canvas = cx.canvas;
        let focus_id = self.focus_id;
        let Some(control) = cx.control.as_deref_mut() else {
            paint_placeholder(canvas, bounds, self.name, "This control has no Default: it cannot be created from a view");
            return;
        };
        {
            let core = control.control_core_mut();
            core.hot = hot;
            core.pressed = down;
        }
        let as_canvas: &dyn Canvas = canvas;
        self.preferred.set(Some(control.get_preferred_size(as_canvas, Size { width: bounds.right - bounds.left, height: bounds.bottom - bounds.top })));
        // Background (unless OPAQUE), paint (raising Paint to the element's handler), the paint
        // buffer — crate::component::paint.
        let handler = self.on_paint.as_deref();
        let mut sink = crate::node::QuietSink { handlers: &mut *cx.handlers, vm: &mut *cx.vm, sender: &cx.sender, focus_id, handler };
        crate::component::paint_control(control, canvas, bounds, state, Some(&mut sink), handler.is_some());
        // The pointer shape and the tooltip of the part under the pointer (`Control::cursor_at`,
        // `Control::tool_tip_at`), over the element's own `Cursor` / `ToolTip`.
        if let (true, None, Some((slot, _)), Some(services)) = (hot, cx.design.as_ref(), cx.sender.clone(), cx.services.as_deref_mut()) {
            let (x, y) = (frame.mouse.0 - bounds.left, frame.mouse.1 - bounds.top);
            if let Some(cursor) = control.cursor_at(x, y) {
                services.offer_cursor(cursor);
            }
            if let Some(tip) = control.tool_tip_at(x, y).filter(|t| !t.trim().is_empty()) {
                services.offer_tooltip(tip, crate::clip::visible_client(crate::common::to_client(bounds)), &slot.id);
            }
        }
        // The parts it paints itself, in the accessibility tree under its element (a grid's tiles).
        if let (None, Some((slot, _)), Some(services)) = (cx.design.as_ref(), cx.sender.clone(), cx.services.as_deref_mut()) {
            let parent = crate::common::access_id(&slot.id);
            for (i, part) in control.accessible_parts().into_iter().enumerate() {
                let r = part.bounds;
                // Cut to what the containers' clips leave visible (`crate::clip`).
                let c = crate::clip::visible_client(crate::common::to_client(Rect::new(bounds.left + r.left, bounds.top + r.top, bounds.left + r.right, bounds.top + r.bottom)));
                services.access.push(host::access::AccessNode {
                    id: crate::common::access_id(&format!("{}/part{i}", slot.id)),
                    parent: Some(parent),
                    role: part.role,
                    name: part.name,
                    description: String::new(),
                    value: None,
                    bounds: (c.left, c.top, c.right, c.bottom),
                    focusable: false,
                    disabled: !enabled,
                    checked: None,
                    clickable: false,
                    read_only: true,
                    access_key: None,
                    expanded: None,
                });
            }
        }
        // A control that invalidated itself while painting (an animation) asks for the next frame.
        if control.invalidated_rect().is_some() || control.control_core().update_requested {
            host::request_repaint_after(0);
        }
    }
}

// ── PaintBox ────────────────────────────────────────────────────────────────────────────────

/// `<PaintBox>` (EVT-8): a surface its `OnPaint` handler draws on with the Graphics API. Painted
/// through its class (`controls::PaintBox`) like a custom control — background (`BackColor`) then
/// `on_paint`, which raises `Paint` with the surface lent — and never buffered while it has a
/// handler (the handler reads the view model, which may change at any frame).
pub struct PaintBoxNode {
    on_paint: Option<String>,
}

impl PaintBoxNode {
    pub fn new(on_paint: Option<String>) -> Self {
        Self { on_paint }
    }
}

impl ViewNode for PaintBoxNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size { width: 200.0, height: 100.0 }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas = cx.canvas;
        let focus_id = cx.sender.as_ref().and_then(|(slot, _)| slot.focus_id);
        let handler = self.on_paint.as_deref();
        let enabled = cx.control.as_deref().is_none_or(|c| c.enabled()) && !crate::common::is_disabled();
        let state = WidgetState::REST.disabled(!enabled);
        let mut sink = crate::node::QuietSink { handlers: &mut *cx.handlers, vm: &mut *cx.vm, sender: &cx.sender, focus_id, handler };
        match cx.control.as_deref_mut() {
            Some(control) => {
                crate::component::paint_control(control, canvas, bounds, state, Some(&mut sink), handler.is_some());
            }
            None => {
                use crate::component::RaiseSink;
                let g = kubuno_ui::graphics::Graphics::new(canvas);
                crate::events::PaintEventArgs::lend(&g, bounds, |args| sink.raise("OnPaint", args));
            }
        }
    }
}

// ── User control ────────────────────────────────────────────────────────────────────────────

thread_local! {
    /// How many user controls are being built inside each other (a user control whose view uses
    /// itself would recurse forever).
    static BUILD_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Whether the view being built is the view of a user control used by another view (not the view itself being
/// designed or run): in the designer, its design-time attributes stay out (they would cover the data its instance
/// is given, every row of a `<Repeater>` showing the same sample) and its controls show as they would at run time,
/// like a Windows Forms user control dropped on a form.
pub(crate) fn inside_user_control() -> bool {
    BUILD_DEPTH.with(|d| d.get() > 0)
}

/// The deepest nesting of user controls a view may build.
const MAX_USER_CONTROL_DEPTH: u32 = 8;

/// A user control (see the module doc).
pub struct UserControlNode {
    name: &'static str,
    root: Option<Box<dyn ViewNode>>,
    /// The inner view's design size (its root's `DesignWidth` × `DesignHeight`).
    design_size: Option<(f32, f32)>,
    /// Why the inner view could not be built (shown in the element's box).
    error: Option<String>,
    handlers: HandlerTable,
    events: Vec<ViewEvent>,
    /// The layout map the inner view records into at design time (discarded: the designer selects
    /// the user control as one element), so its elements know they are designed.
    design_scratch: crate::design::LayoutMap,
    loaded: bool,
    /// The context menus its own view declares (`<ContextMenu x:Name="…">`), offered to the window each frame.
    menus: Vec<std::rc::Rc<crate::window::MenuSpec>>,
    /// The handler its own view's root names for `Load` (`<UserControl x:Class="…" OnLoad="…">`): a
    /// method of the user control, run when it loads, before the `Load` of the element using it.
    load_handler: Option<String>,
    /// The inner elements of its view's `AcceptButton` / `CancelButton` (their ids), clicked by an
    /// Enter / Escape nobody used while the focus is inside the user control.
    accept: Option<String>,
    cancel: Option<String>,
    /// The focus ids of the inner elements (whether the focus is inside).
    inner_focus: Vec<FocusId>,
    /// The inner button to click at the next frame.
    pending_click: Option<String>,
    /// The scope of its view's ids (what `ActiveControl` names are resolved in).
    focus_scope: Option<u64>,
}

/// The id of the element named `name` in the view `root`, if any.
fn element_id(root: &crate::ast::Element, name: &str) -> Option<String> {
    use crate::ast::AstNode;
    root.syntax()
        .descendants()
        .filter_map(crate::ast::Element::cast)
        .find(|e| e.attribute("x:Name").and_then(|a| a.value()).as_deref() == Some(name))
        .map(|e| e.stable_id())
}

/// The `build` of a linked user control: compiles its embedded view.
pub fn user_control_build(props: &Props<'_>, _cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let name = props.meta().name;
    let mut node = UserControlNode {
        name,
        root: None,
        design_size: None,
        error: None,
        handlers: HandlerTable::new(),
        events: Vec::new(),
        design_scratch: crate::design::LayoutMap::new(),
        loaded: false,
        load_handler: None,
        menus: Vec::new(),
        accept: None,
        cancel: None,
        inner_focus: Vec::new(),
        pending_click: None,
        focus_scope: None,
    };
    let info = project::project_info(name);
    let Some(view) = info.and_then(|i| i.view) else {
        node.error = Some("its view was not compiled in".to_string());
        return Ok(Box::new(node));
    };
    // Its relative paths (`d:ItemsSource="design/rows.json"`, images) are next to its own `.kbcontrol`, which may be in
    // another folder than the view nesting it; unknown or missing (another machine), the nesting view's folder.
    let view_dir = info.and_then(|i| i.view_dir).map(std::path::Path::new).filter(|d| d.is_dir());
    let depth = BUILD_DEPTH.with(|d| d.get());
    if depth >= MAX_USER_CONTROL_DEPTH {
        return Err(BuildError::new(format!("`<{name}>` is nested more than {MAX_USER_CONTROL_DEPTH} user controls deep (does its view use itself?)"), props.element().name_range()));
    }
    BUILD_DEPTH.with(|d| d.set(depth + 1));
    // Its view's elements get ids of their own (scoped by this element's id): routed, focused and announced to
    // assistive technology apart from the page's and from the other instances'.
    let scope = Some(crate::compile::scope_hash(&crate::compile::scoped_id(props.element().stable_id())));
    node.focus_scope = scope;
    let compiled = crate::compile::with_item_scope(scope, || match view_dir {
        Some(dir) => crate::compile::compile_in(view, Some(dir)),
        None => crate::compile::compile(view),
    });
    BUILD_DEPTH.with(|d| d.set(depth));
    match compiled {
        Ok(compiled) => {
            node.design_size = compiled.design_size;
            node.menus = compiled.menus.iter().cloned().map(std::rc::Rc::new).collect();
            node.root = Some(compiled.root);
            // `AcceptButton` / `CancelButton` of the user control's own view (a dialog body made of a
            // user control), and the names that tell the focus is inside it.
            use crate::ast::AstNode;
            let parse = crate::syntax::parse(view);
            if let Some(root) = crate::ast::Document::cast(parse.syntax()).and_then(|d| d.root_element()) {
                let attr = |n: &str| root.attribute(n).and_then(|a| a.value()).filter(|v| !v.trim().is_empty());
                node.load_handler = attr("OnLoad").map(|h| h.trim().to_string());
                crate::compile::with_item_scope(scope, || {
                    node.accept = attr("AcceptButton").and_then(|b| element_id(&root, &b)).map(crate::compile::scoped_id);
                    node.cancel = attr("CancelButton").and_then(|b| element_id(&root, &b)).map(crate::compile::scoped_id);
                    node.inner_focus = root
                        .syntax()
                        .descendants()
                        .filter_map(crate::ast::Element::cast)
                        .filter_map(|e| e.attribute("x:Name").and_then(|a| a.value()))
                        .map(|n| crate::compile::scoped_focus(FocusId::of(&n)))
                        .collect();
                });
            }
        }
        Err(diagnostics) => {
            let first = diagnostics.first().map(|d| format!("line {}: {}", d.line, d.message)).unwrap_or_default();
            node.error = Some(format!("its view does not compile ({first})"));
        }
    }
    Ok(Box::new(node))
}

impl ViewNode for UserControlNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        let (width, height) = self.design_size.unwrap_or((200.0, 100.0));
        Size { width, height }
    }

    /// Its own view is clipped to the instance (WinForms: a user control clips its children like any control),
    /// whatever the size of the view's design.
    fn clips_children(&self) -> bool {
        true
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas = cx.canvas;
        let (Some(root), Some(control)) = (self.root.as_mut(), cx.control.as_deref_mut()) else {
            let why = self.error.clone().unwrap_or_else(|| "it has no Default: it cannot be created from a view".to_string());
            paint_placeholder(canvas, bounds, self.name, &why);
            return;
        };
        let designing = cx.design.is_some();
        // Load, once, before its first paint (WinForms `UserControl.OnLoad`) — in the designer too,
        // as Windows Forms does: the user control's own code runs there with `design_mode()` true
        // (sample data for the designer), while the handlers of the view using it never run there.
        if !self.loaded {
            self.loaded = true;
            if let Err(why) = load_user_control(control, self.load_handler.as_deref(), bounds, designing) {
                self.error = Some(why);
            }
        }
        if let Some(why) = self.error.as_deref().filter(|_| designing) {
            paint_placeholder(canvas, bounds, self.name, why);
            return;
        }
        self.design_scratch.clear();
        // `ActiveControl` set from its code: the control of that name inside takes the focus.
        if !designing {
            if let Some(name) = control.as_container_control_mut().and_then(|c| c.container_control_core_mut().focus_request.take()) {
                let id = crate::compile::with_item_scope(self.focus_scope, || crate::compile::scoped_focus(FocusId::of(&name)));
                cx.focus.focus_visibly(id);
            }
        }
        let Some(vm) = control.as_component_mut().kubuno_view_model() else {
            paint_placeholder(canvas, bounds, self.name, "not a user control");
            return;
        };
        // The default button chosen at the last frame is clicked now (as its mnemonic would be).
        if let (Some(id), Some(services)) = (self.pending_click.take(), cx.services.as_deref_mut()) {
            services.activate.push(id);
        }
        let submitted_before = cx.services.as_deref().is_some_and(|s| s.submitted);
        // Its own view's elements are routed like the page's (a control inside a user control gets its mouse, wheel
        // and key events), their handlers dispatched to the user control first (`crate::events::router::DispatchScope`).
        let scope = cx.router.as_deref_mut().and_then(|router| {
            let scope = router.control_scope_of_last()?;
            router.push_scope(scope.clone());
            Some(scope)
        });
        let scoped = scope.is_some();
        let menus_before = cx.services.as_deref().map_or(0, |s| s.context_menus.len());
        let start = cx.router.as_deref().map(|r| r.registered_count());
        let mut inner = PaintCx {
            canvas,
            frame: cx.frame,
            vm,
            focus: &mut *cx.focus,
            handlers: &mut self.handlers,
            events: &mut self.events,
            design: if designing { Some(&mut self.design_scratch) } else { None },
            router: cx.router.as_deref_mut(),
            sender: None,
            control: None,
            services: cx.services.as_deref_mut(),
            activate: false,
        };
        root.paint(&mut inner, bounds);
        if let Some(router) = cx.router.as_deref_mut() {
            if scoped {
                router.pop_scope();
            }
            // Its view's root is the user control itself: the pointer over its surface reaches it.
            if let Some(start) = start {
                router.absorb_view_root(start);
            }
        }
        // Its own context menus: the elements of its view that name one open it, its handlers run on it.
        if let (false, Some(services)) = (designing || self.menus.is_empty(), cx.services.as_deref_mut()) {
            let tag = format!("{:p}", self as *const Self);
            let key = |name: &str| format!("{name}@{tag}");
            for entry in services.context_menus.iter_mut().skip(menus_before) {
                if self.menus.iter().any(|m| m.name == entry.2) {
                    entry.2 = key(&entry.2);
                }
            }
            for menu in &self.menus {
                services.local_menus.push(crate::common::LocalMenu { key: key(&menu.name), name: menu.name.clone(), spec: menu.clone(), scope: scope.clone() });
            }
        }
        self.events.clear();
        // `AcceptButton` / `CancelButton` of the user control's view: an Enter / Escape its controls left,
        // while the focus is inside it, clicks that button (before the window's own default buttons).
        if designing || (self.accept.is_none() && self.cancel.is_none()) {
            return;
        }
        if !cx.focus.focused().is_some_and(|f| self.inner_focus.contains(&f)) {
            return;
        }
        // Enter in one of its single-line fields (a submit): its own accept button, not the window's.
        if let (Some(a), Some(services)) = (&self.accept, cx.services.as_deref_mut()) {
            if services.submitted && !submitted_before {
                services.submitted = false;
                self.pending_click = Some(a.clone());
                host::request_repaint_after(0);
                return;
            }
        }
        use kubuno_controls::host::{vk, Modifiers};
        let chosen = match (&self.accept, &self.cancel) {
            (Some(a), _) if host::take_key(vk::ENTER, Modifiers::NONE) > 0 => Some(a.clone()),
            (_, Some(c)) if host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 => Some(c.clone()),
            _ => None,
        };
        if let Some(id) = chosen {
            self.pending_click = Some(id);
            host::request_repaint_after(0);
        }
    }
}

/// Loads a user control: its own view's `OnLoad` handler (a method of the user control: its
/// `InitializeComponent` subscribed it first, as Windows Forms does), then `on_load` (its class's
/// override, the `Load` subscribers, the handler of the element using it). In the designer, a panic
/// of the user control's code is caught and reported in its box (`Err`), as the Windows Forms
/// designer shows a control that failed to load, instead of ending the design surface.
pub(crate) fn load_user_control(control: &mut dyn crate::component::Control, handler: Option<&str>, bounds: Rect, designing: bool) -> Result<(), String> {
    let mut run = || {
        let component = control.as_component_mut();
        if let Some(handler) = handler.filter(|h| !h.is_empty()) {
            let class = component.class_name();
            let name = component.display_name().to_string();
            if let Some(vm) = component.kubuno_view_model() {
                let sender = crate::events::ElementRef { name: Some(&name), element: class, id: "", bounds, focus_id: None, attributes: &[] };
                if !vm.dispatch_event(handler, &sender, &mut EmptyEventArgs) {
                    tracing::warn!("`OnLoad=\"{handler}\"` of the user control `{class}`: no such method in its `#[event_handlers]` impl");
                }
            }
        }
        if let Some(uc) = control.as_component_mut().as_user_control_mut() {
            let class = uc.class_name();
            uc.on_load(&mut EventCx::new(&mut EmptyEventArgs).from_class(class));
        }
    };
    if !designing {
        run();
        return Ok(());
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).map_err(|panic| {
        let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
        format!("its Load failed: {message}")
    })
}

// ── Non-visual components ───────────────────────────────────────────────────────────────────

/// A component that paints nothing (a `#[kubuno(extends = Component)]` class of the application).
pub struct NonVisualNode;

/// The `build` of a non-visual component.
pub fn non_visual_build(_props: &Props<'_>, _cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    Ok(Box::new(NonVisualNode))
}

impl ViewNode for NonVisualNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size { width: 0.0, height: 0.0 }
    }

    fn paint(&mut self, _cx: &mut PaintCx<'_>, _bounds: Rect) {}
}

/// `<Timer Interval="1000" Enabled="true" OnTick="…"/>` (WinForms `Timer`): raises `Tick` every
/// `Interval` milliseconds while `Enabled` — never in the designer. The view keeps running
/// frames for it (the host is woken for the next tick, minimised included).
pub struct TimerNode {
    pub interval: PropSource<f32>,
    pub enabled: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_tick: Option<String>,
    next_due: Option<Instant>,
}

impl TimerNode {
    pub fn new(interval: PropSource<f32>, enabled: PropSource<bool>, focus_id: Option<FocusId>, on_tick: Option<String>) -> Self {
        Self { interval, enabled, focus_id, on_tick, next_due: None }
    }

    /// Whether a tick is due at `now` (and schedules the next one); `None` means disabled.
    pub(crate) fn step(&mut self, now: Instant, enabled: bool, interval_ms: f32) -> Option<(bool, Duration)> {
        if !enabled {
            self.next_due = None;
            return None;
        }
        let interval = Duration::from_micros((interval_ms.max(1.0) * 1000.0).round() as u64);
        let due = *self.next_due.get_or_insert(now + interval);
        if now >= due {
            // Late ticks coalesce into one, like WM_TIMER.
            self.next_due = Some(now + interval);
            Some((true, interval))
        } else {
            Some((false, due - now))
        }
    }
}

impl ViewNode for TimerNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size { width: 0.0, height: 0.0 }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, _bounds: Rect) {
        if cx.design.is_some() {
            self.next_due = None;
            return;
        }
        let enabled = self.enabled.resolve(cx.vm);
        let interval = self.interval.resolve(cx.vm);
        match self.step(Instant::now(), enabled, interval) {
            Some((true, next)) => {
                cx.fire("OnTick", self.focus_id, self.on_tick.as_deref(), ViewEventKind::Other { name: "Tick", args: std::rc::Rc::new(EmptyEventArgs) }, &mut EmptyEventArgs);
                host::request_wake_after(next.as_millis().min(u128::from(u32::MAX)) as u32);
            }
            Some((false, wait)) => host::request_wake_after((wait.as_millis() as u32).max(1)),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_ticks_on_its_interval_and_coalesces_late_ticks() {
        let mut t = TimerNode::new(PropSource::Literal(100.0), PropSource::Literal(true), None, None);
        let t0 = Instant::now();
        assert_eq!(t.step(t0, true, 100.0).map(|(tick, _)| tick), Some(false));
        assert_eq!(t.step(t0 + Duration::from_millis(50), true, 100.0).map(|(tick, _)| tick), Some(false));
        assert_eq!(t.step(t0 + Duration::from_millis(100), true, 100.0).map(|(tick, _)| tick), Some(true));
        // 350 ms late: one tick, then the next is one interval after it.
        assert_eq!(t.step(t0 + Duration::from_millis(450), true, 100.0).map(|(tick, _)| tick), Some(true));
        assert_eq!(t.step(t0 + Duration::from_millis(500), true, 100.0).map(|(tick, _)| tick), Some(false));
        assert!(t.step(t0 + Duration::from_millis(600), false, 100.0).is_none());
        // Re-enabled: counting starts again.
        assert_eq!(t.step(t0 + Duration::from_millis(700), true, 100.0).map(|(tick, _)| tick), Some(false));
    }
}
