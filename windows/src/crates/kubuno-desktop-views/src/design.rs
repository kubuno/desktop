//! The Rust half of the design mode `vskubuno/docs/DESIGNER.md`'s work
//! package DSG-6 asks for: a frame-local **layout map** (stable element id +
//! painted bounds + parent id + layout kind, recorded during an ordinary
//! paint pass), hit-testing over it, and the pure request-generation logic
//! (selection, nudge, delete) a design surface (`examples/view_embed.rs`)
//! drives from real input. Deliberately host-agnostic: nothing here reads
//! `kubuno_desktop_controls::host`'s global input state directly — a caller (the
//! example, in production; a plain struct literal, in this module's own
//! tests) hands in a small [`DesignKeyInput`]/mouse position instead, so
//! request generation is unit-testable with no live window or `Canvas`, per
//! `DESIGNER.md` §6's DSG-6 test strategy ("Hit-test math is unit-testable
//! headless … adorner rendering needs a visual check").
//!
//! ## The layout map
//!
//! [`DesignSlot`] is a [`crate::node::ViewNode`] decorator: `crate::compile::
//! build_node` wraps EVERY element it builds in one, regardless of which
//! family declared it (`crate::node`'s five, or any `registry::families::*`
//! component) — so every element gets a stable id and a recorded bounds
//! without each `ViewNode` impl having to do it itself. Its `paint` records
//! one [`LayoutEntry`] into the frame's [`LayoutMap`] (when one is present —
//! [`crate::node::PaintCx::design`] is `None` outside design mode, so the
//! only added cost is one `if let` branch per element, no allocation) and
//! then delegates unchanged to the wrapped node — geometry, focus, binding,
//! events all still flow exactly as they did before this module existed.
//!
//! ## Element ids and layout kind
//!
//! An entry's `id` is [`crate::ast::Element::stable_id`] — the same dotted
//! child-ordinal path `kubuno-desktop-views-ls`'s `kubuno/applyEdit` bridge resolves
//! (DSG-2, `DESIGNER.md` §8): the two processes agree on "which element"
//! without either walking the other's tree. `parent_id` is the same split
//! DSG-2's own `edit_bridge::split_parent` does (`"2.0.3"` → `"2.0"`, `"3"` →
//! `""`, `""` → no parent) — [`parent_id_of`] here is the DSG-6 side of that
//! shared, id-only contract. `layout` is the [`crate::registry::LayoutKind`]
//! of the element's own PARENT container (not the element's own, when it is
//! itself a container) — what decides its adorner per `DESIGNER.md` §4: a
//! direct child of a `LayoutKind::DockAnchor` container (`<Panel>`) gets the
//! 8 resize handles, everything else gets the plain flow outline.

use kubuno_desktop_ui::Rect;
use serde::{Deserialize, Serialize};

use crate::ast;
use crate::node::{PaintCx, ViewNode};
use crate::registry::{self, ChildrenModel, LayoutKind};

// ─────────────────────────────────────────────────────────────────────────
// Layout map
// ─────────────────────────────────────────────────────────────────────────

/// One element's own recorded frame: its stable id, its parent's (`None`
/// only for the document root), the bounds it was actually painted into, and
/// the [`LayoutKind`] of the CONTAINER that positioned it (its parent's
/// layout, not its own — see the module doc).
#[derive(Clone)]
pub struct LayoutEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub bounds: Rect,
    pub layout: LayoutKind,
    /// Whether the element itself accepts children (its registry `ChildrenModel` is not `None`): a
    /// press on its empty area starts a marquee selection inside it (`DESIGNER.md` §13).
    pub container: bool,
    /// `Locked="true"`: selected, but never moved, resized or nudged (Windows Forms' `Locked`).
    pub locked: bool,
    /// The clip its containers put on it when its box crosses it (`crate::clip`), in the coordinates of `bounds`:
    /// only the part of `bounds` inside it is hit by the pointer. `None` when nothing clips it. The adorners still
    /// frame the whole `bounds`, as the Windows Forms designer frames a control its parent cuts.
    pub clip: Option<Rect>,
}

impl LayoutEntry {
    /// What the pointer can hit of the element: `bounds`, cut by `clip`.
    pub fn hit_bounds(&self) -> Rect {
        match self.clip {
            Some(clip) => crate::clip::intersect(self.bounds, clip),
            None => self.bounds,
        }
    }
}

/// A frame's worth of [`LayoutEntry`] — built fresh every frame design mode
/// is on (`crate::runtime::Runtime::frame_with_design`), read afterwards for
/// hit-testing and adorner geometry. Entries land in paint order: a
/// container is always pushed before its own children (`DesignSlot::paint`
/// records itself, then delegates, and only a container's own delegate walks
/// further into its children), so a later entry is always at least as deep
/// as, or a later sibling of, an earlier one — [`LayoutMap::hit_test`]
/// exploits exactly that ordering.
#[derive(Default)]
pub struct LayoutMap {
    entries: Vec<LayoutEntry>,
}

impl LayoutMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops every entry — called once at the start of a frame that records
    /// a fresh map, so a design surface never hit-tests against a stale
    /// frame's geometry after a resize/reflow.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn push(&mut self, entry: LayoutEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[LayoutEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&LayoutEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Point → **deepest** element id, per `DESIGNER.md`'s DSG-6 scope
    /// ("Hit-test: point → deepest element"). Scans in REVERSE paint order:
    /// since a container is always recorded before its own children (see the
    /// struct doc), the last entry whose bounds contain the point is always
    /// the most nested one that does (and, among overlapping siblings, the
    /// one painted last / on top) — no separate depth bookkeeping needed.
    /// Skips a degenerate (zero-or-negative-area) entry: "appropriately skip
    /// non-visual items" — an element that measured to nothing never steals a
    /// click from whatever is actually under the pointer.
    ///
    /// Only the part of an element its containers leave visible is hit ([`LayoutEntry::hit_bounds`]): a button
    /// wider than its panel is not selected by a click beside the panel.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&LayoutEntry> {
        self.entries.iter().rev().find(|e| {
            let b = e.hit_bounds();
            b.right > b.left && b.bottom > b.top && b.contains(x, y)
        })
    }
}

/// The inverse of the id's own construction (`Element::stable_id`'s doc):
/// splits `id` at its last `.` to give `(parent_id, index)`, keeping only the
/// parent half — `"2.0.3"` → `Some("2.0")`, `"3"` → `Some("")`, `""` (the
/// document root) → `None`. Mirrors `kubuno-desktop-views-ls`'s `edit_bridge::
/// split_parent` (`DESIGNER.md` §8), the DSG-2 side of the same id-only
/// contract, kept here as a free function since [`LayoutEntry`] stores the
/// result directly rather than recomputing it from the id string on every
/// lookup.
/// (The function is `kubuno_desktop_views_syntax::ids::parent_id_of`, re-exported: it is pure id arithmetic,
/// shared by every target (WV-1).)
pub use kubuno_desktop_views_syntax::ids::parent_id_of;

/// Wraps any [`ViewNode`] with its element's stable id and parent layout —
/// see the module doc for why `crate::compile::build_node` applies this to
/// every element uniformly instead of each component family recording
/// itself. `measure`/`measure_for_width`/`intrinsic_width` all delegate
/// unchanged: only `paint` does anything extra.
///
/// It also owns the element's **class instance** (EVT-7a, `crate::component`): a
/// [`crate::controls::Button`] for a `<Button>`… Every paint keeps its core in step with the
/// element (name, bounds, focus, design mode), hands it to the node's context so the node's own
/// events are delivered through its `on_…` methods, and registers it with the input router,
/// which delivers the routed events (MouseDown, KeyDown, GotFocus…) the same way. The instance
/// is disposed with the slot (a hot reload rebuilds the tree).
pub struct DesignSlot {
    id: String,
    parent_layout: LayoutKind,
    container: bool,
    inner: Box<dyn ViewNode>,
    /// The element as the input router (EVT-2) sees it: registered with
    /// [`crate::events::router::InputRouter`] every paint, and the sender of the events its
    /// node raises while it paints.
    events: Option<std::rc::Rc<crate::events::router::SlotEvents>>,
    /// The element's class instance (see the struct doc).
    control: Option<std::rc::Rc<std::cell::RefCell<dyn crate::component::Component>>>,
    /// An application class's own properties, set on its instance every paint (EVT-7b).
    custom_props: Vec<CustomProp>,
    /// The properties the element inherits and sets (`Enabled`, colours, size limits…), honoured
    /// around its node's paint (`crate::common`); `None` when it sets none.
    common: Option<Box<crate::common::CommonProps>>,
    /// The element's `TabIndex` (0 when not written) when the view orders its Tab stops; `None` when
    /// no element of the view has a `TabIndex` (the plain paint order).
    tab_index: Option<i32>,
    /// The element's registry entry (its accessibility role, whether it clicks).
    meta: Option<&'static crate::registry::ComponentMeta>,
    /// The paths its properties are bound to (read once from its attributes, DATA-2).
    bound_paths: std::cell::OnceCell<std::rc::Rc<[String]>>,
    /// While the left button is held, whether the press started over the visible part of the element when its
    /// containers clip it (`crate::clip`): a drag that started there keeps the pointer outside it, as a captured
    /// mouse does. `None` while the button is up.
    clip_press: Option<bool>,
}

/// One property of an application class (EVT-7b) read from its element: a literal, or a binding
/// resolved every frame; [`DesignSlot`] sets it on the class instance before the element paints.
pub struct CustomProp {
    name: &'static str,
    source: CustomSource,
    /// The value applied at the last paint (a change invalidates the control).
    last: std::cell::RefCell<Option<crate::binding::Value>>,
}

enum CustomSource {
    Literal(crate::binding::Value),
    Bound(crate::binding::BindingSpec),
}

impl CustomProp {
    /// The value of `property` on `element`, `None` when the element does not set it.
    pub fn read(element: &ast::Element, property: &crate::registry::PropertyMeta) -> Option<Self> {
        use crate::binding::Value;
        let raw = element.attribute(property.name)?.value()?;
        let source = if crate::binding::is_binding_expr(&raw) {
            CustomSource::Bound(crate::binding::parse_binding(&raw)?)
        } else {
            CustomSource::Literal(match property.kind {
                crate::registry::PropKind::Bool => Value::Bool(raw.trim() == "true"),
                crate::registry::PropKind::F32 => Value::F32(raw.trim().parse().ok()?),
                _ => Value::Str(raw),
            })
        };
        Some(Self { name: property.name, source, last: std::cell::RefCell::new(None) })
    }

    /// The property's value this frame (`None`: a binding whose path is unset).
    pub fn resolve(&self, vm: &dyn crate::binding::ViewModel) -> Option<crate::binding::Value> {
        match &self.source {
            CustomSource::Literal(v) => Some(v.clone()),
            // A resource reference (`Text="{Res manage}"`) is looked up like a built-in property's.
            CustomSource::Bound(spec) => crate::resources::get(vm, spec),
        }
    }

    /// The attribute name.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl DesignSlot {
    pub fn new(id: String, parent_layout: LayoutKind, inner: Box<dyn ViewNode>) -> Self {
        Self {
            id,
            parent_layout,
            container: false,
            inner,
            events: None,
            control: None,
            custom_props: Vec::new(),
            common: None,
            tab_index: None,
            meta: None,
            bound_paths: std::cell::OnceCell::new(),
            clip_press: None,
        }
    }

    /// The inherited properties the element sets (see [`crate::common::CommonProps`]).
    pub fn with_common(mut self, common: Option<crate::common::CommonProps>) -> Self {
        self.common = common.map(Box::new);
        self
    }

    /// The element's place in the Tab order, when the view orders its Tab stops.
    pub fn with_tab_index(mut self, index: Option<i32>) -> Self {
        self.tab_index = index;
        self
    }

    /// The element's registry entry.
    pub fn with_meta(mut self, meta: &'static crate::registry::ComponentMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Attaches the element's event description (see [`crate::events::router::SlotEvents`]).
    pub fn with_events(mut self, events: std::rc::Rc<crate::events::router::SlotEvents>) -> Self {
        self.events = Some(events);
        self
    }

    /// Records the element as a container (it accepts children) - see [`LayoutEntry::container`].
    pub fn with_container(mut self, container: bool) -> Self {
        self.container = container;
        self
    }

    /// Attaches the element's class instance (see the struct doc).
    pub fn with_control(mut self, control: std::rc::Rc<std::cell::RefCell<dyn crate::component::Component>>) -> Self {
        self.control = Some(control);
        self
    }

    /// The application class's own properties (see [`CustomProp`]).
    pub fn with_custom_props(mut self, props: Vec<CustomProp>) -> Self {
        self.custom_props = props;
        self
    }

    /// The element's class instance, if it has one.
    pub fn control(&self) -> Option<&std::rc::Rc<std::cell::RefCell<dyn crate::component::Component>>> {
        self.control.as_ref()
    }

    /// Brings the class instance's core in step with the element before it paints: its site
    /// (`x:Name`, design mode), name, bounds and focus; the first paint creates it
    /// (`Control::create_control`).
    fn sync_control(component: &mut dyn crate::component::Component, slot: Option<&crate::events::router::SlotEvents>, bounds: Rect, design_mode: bool, focused: bool) {
        use crate::component::Site;
        // An unnamed component keeps the name the view gave it when it was built (`bindingSource2`).
        let name = slot.and_then(|s| s.name.clone()).or_else(|| component.site().map(|s| s.name.clone())).unwrap_or_default();
        if component.site().is_none_or(|s| s.design_mode != design_mode || s.name != name) {
            component.set_site(Some(Site { name: name.clone(), design_mode, container: None }));
        }
        let Some(control) = component.as_control_mut() else { return };
        let core = control.control_core_mut();
        if core.name != name {
            core.name = name;
        }
        core.bounds = bounds;
        core.focus_id = slot.and_then(|s| s.focus_id);
        core.focused = focused;
        control.create_control();
    }
}

impl Drop for DesignSlot {
    /// The element leaves the view (a hot reload, the view closing): its class instance is
    /// disposed (`Component::dispose`, which raises `Disposed`).
    fn drop(&mut self) {
        if let Some(control) = &self.control {
            // A named component kept by a hot reload is owned by the new tree too: not disposed.
            if std::rc::Rc::strong_count(control) > 1 {
                return;
            }
            if let Ok(mut c) = control.try_borrow_mut() {
                c.dispose();
            }
        }
    }
}

impl ViewNode for DesignSlot {
    fn is_hidden(&self, vm: &dyn crate::binding::ViewModel) -> bool {
        self.common.as_ref().is_some_and(|common| !common.visible(vm))
    }

    fn measure(&self, c: &dyn kubuno_desktop_ui::Canvas, vm: &dyn crate::binding::ViewModel) -> kubuno_desktop_ui::Size {
        match &self.common {
            Some(common) if !common.visible(vm) => kubuno_desktop_ui::Size::new(0.0, 0.0),
            Some(common) => common.measure(common.measuring(c, vm, |c| self.inner.measure(c, vm)), self.parent_layout),
            None => self.inner.measure(c, vm),
        }
    }

    fn measure_for_width(&self, c: &dyn kubuno_desktop_ui::Canvas, vm: &dyn crate::binding::ViewModel, width: f32) -> kubuno_desktop_ui::Size {
        match &self.common {
            Some(common) if !common.visible(vm) => kubuno_desktop_ui::Size::new(0.0, 0.0),
            Some(common) => common.measure(common.measuring(c, vm, |c| self.inner.measure_for_width(c, vm, width)), self.parent_layout),
            None => self.inner.measure_for_width(c, vm, width),
        }
    }

    fn intrinsic_width(&self, c: &dyn kubuno_desktop_ui::Canvas, vm: &dyn crate::binding::ViewModel) -> Option<f32> {
        match &self.common {
            Some(common) if !common.visible(vm) => Some(0.0),
            Some(common) => common.measuring(c, vm, |c| self.inner.intrinsic_width(c, vm)).map(|w| common.measure(kubuno_desktop_ui::Size::new(w, 0.0), self.parent_layout).width),
            None => self.inner.intrinsic_width(c, vm),
        }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        // Visible="false": not painted, not routed, not focusable, not announced (the designer still
        // shows it). The element's box inside what its container gave: margin, size limits.
        let bounds = match &self.common {
            Some(common) if !common.visible(&*cx.vm) => return,
            Some(common) => common.place(bounds, self.parent_layout),
            None => bounds,
        };
        // A control of the base view of an inherited view (not in the document designed): never selected, moved or
        // edited there, like the Windows Forms designer's locked inherited controls.
        // The title bar's standard items (`HEADER_ITEM_PREFIX`) are not in the document either: never selected.
        let inherited = self.id.starts_with(INHERITED_LOCKED_PREFIX) || self.id.starts_with(INHERITED_INNER_PREFIX) || self.id.starts_with(HEADER_ITEM_PREFIX);
        // The clip its containers put on it (WinForms clips every control to its parent's box, `crate::clip`):
        // `Some(clip)`, in the current content coordinates, when its box crosses it.
        let cut = crate::clip::cut(bounds);
        if let Some(map) = cx.design.as_mut().filter(|_| !inherited) {
            map.push(LayoutEntry {
                id: self.id.clone(),
                parent_id: parent_id_of(&self.id),
                bounds,
                layout: self.parent_layout,
                container: self.container,
                locked: self.common.as_ref().is_some_and(|c| c.locked()),
                clip: cut,
            });
        }
        // The pointer over the clipped-away part of the element does not reach it (nor anything in it).
        let masked = cut.and_then(|clip| self.mask_pointer(cx.frame, clip));
        // The paint debug overlay's layout bounds (EVT-8): the element's box, padding and margin.
        // A run-time diagnostic only: never on a design surface, where every control must look
        // exactly as it does at run time (the overlay boxed every Label and TextField of a designed
        // view in cyan when Visual Studio's Debug > Kubuno > Paint debug was left on).
        if cx.design.is_none() && kubuno_desktop_controls::host::paint_debug::layout_enabled() {
            let (margin, padding) = self.common.as_ref().map(|c| c.margin_padding()).unwrap_or((None, None));
            kubuno_desktop_controls::host::paint_debug::note_layout(crate::common::to_client(bounds), padding, margin);
        }
        let design_mode = cx.design.is_some();
        let own_enabled = self.common.as_ref().is_none_or(|c| c.enabled(&*cx.vm));
        let enabled = own_enabled && !crate::common::is_disabled();
        let focus_id = self.events.as_ref().and_then(|s| s.focus_id);
        let focused = focus_id.is_some_and(|id| cx.focus.is_focused(id));
        // The class instance, borrowed for the whole paint of the element (its node's events go
        // through it); a failed borrow (never expected: nothing else holds it while the tree
        // paints) paints the element without it, as before EVT-7a.
        let control_cell = self.control.clone();
        let mut guard = control_cell.as_ref().and_then(|c| c.try_borrow_mut().ok());
        if let Some(component) = guard.as_deref_mut() {
            Self::sync_control(component, self.events.as_deref(), bounds, design_mode, focused);
            if let Some(control) = component.as_control_mut() {
                // `Enabled`, as its class sees it (a disabled container disables it too).
                control.control_core_mut().props.enabled = enabled;
            }
            apply_custom_props(component, &self.custom_props, &*cx.vm);
        }
        let focus_mark = cx.focus.mark();
        if let Some(index) = self.tab_index {
            cx.focus.push_tab_index(index);
        }
        let activate = match (&self.events, cx.services.as_deref_mut()) {
            (Some(slot), Some(services)) if enabled => services.take_activation(&slot.id),
            _ => false,
        };
        let publishes = self.publishes_access();
        if let Some(services) = cx.services.as_deref_mut() {
            services.depth += 1;
            if let (true, Some(slot)) = (publishes, self.events.as_deref()) {
                services.access_parents.push(crate::common::access_id(&slot.id));
            }
        }
        let outer = match &self.events {
            Some(slot) => {
                if let Some(router) = cx.router.as_deref_mut() {
                    router.register_with_control(slot.clone(), bounds, self.control.as_ref());
                    if !enabled {
                        router.disable_last();
                    }
                }
                Some(cx.sender.replace((slot.clone(), bounds)))
            }
            None => None,
        };
        // The only design-time visual on the element itself: a faint dashed outline around a
        // container that is otherwise invisible (no surface, border or background), so it can be
        // found and dropped into - never around a control, and never around the view's root.
        // Painted under the container's children, like the dotted border Windows Forms draws on a
        // borderless Panel's own surface: a child touching the edge covers it.
        let canvas = cx.canvas;
        // Crossing its containers' clip: everything it paints is cut to it (`crate::clip`).
        if let Some(clip) = cut {
            canvas.push_clip(&clip);
        }
        {
            // The pointer away from it when it is over its clipped-away part.
            let mut pcx = match masked.as_ref() {
                Some(frame) => cx.with_surface(canvas, frame),
                None => cx.reborrow(),
            };
            if design_mode
                && self.container
                && !self.id.is_empty()
                && container_outlines()
                && self.common.as_ref().is_none_or(|c| !c.paints_box())
                && self.inner.is_invisible_container(&*pcx.vm)
            {
                let canvas: &dyn kubuno_desktop_ui::Canvas = canvas;
                paint_container_outline(canvas, bounds);
            }
            // A container (or a user control) clips what it holds to its own box.
            let children_clip = (self.container || self.inner.clips_children()).then(|| crate::clip::Children::push(bounds));
            let inner_node = &mut self.inner;
            let control = guard.as_deref_mut().and_then(|c| c.as_control_mut());
            let paint_node = move |cx: &mut PaintCx<'_>, rect: Rect| {
                let mut inner = cx.reborrow();
                inner.control = control;
                inner.activate = activate;
                inner_node.paint(&mut inner, rect);
            };
            match &self.common {
                Some(common) => common.paint_styled(&mut pcx, bounds, own_enabled, paint_node),
                None => paint_node(&mut pcx, bounds),
            }
            drop(children_clip);
        }
        if design_mode && self.id.starts_with(INHERITED_LOCKED_PREFIX) {
            let canvas: &dyn kubuno_desktop_ui::Canvas = canvas;
            paint_inherited_glyph(canvas, bounds);
        }
        if cut.is_some() {
            canvas.pop_clip();
            // What it and its content registered for the pointer focus is cut to the visible part.
            if let Some(clip) = crate::clip::current() {
                cx.focus.clip_since(focus_mark, clip);
            }
        }
        if let Some(outer) = outer {
            cx.sender = outer;
        }
        // A disabled element and everything in it leave the focus ring; TabStop="false" leaves the
        // Tab order only.
        if !enabled {
            cx.focus.remove_since(focus_mark);
        } else if let (Some(false), Some(id)) = (self.common.as_ref().and_then(|c| c.tab_stop()), focus_id) {
            cx.focus.skip_tab_since(focus_mark, id);
        }
        if self.tab_index.is_some() {
            cx.focus.pop_tab_index();
        }
        if !design_mode {
            self.offer_services(cx, bounds, enabled, focus_id);
        }
        if let Some(services) = cx.services.as_deref_mut() {
            services.depth = services.depth.saturating_sub(1);
            if publishes {
                services.access_parents.pop();
            }
        }
        // Events the class raised outside a dispatch (a declared event's `raise_…`, EVT-7b): its
        // element's handler runs now, the instance released first (the handler may use it, DATA-2).
        // Never in the designer.
        let queued = match guard.as_deref_mut() {
            Some(component) if component.component_core().has_queued() => component.component_core_mut().take_queued(),
            _ => Vec::new(),
        };
        drop(guard);
        if !queued.is_empty() {
            let dispatch = if design_mode { None } else { self.events.as_deref() };
            deliver_taken(queued, dispatch, bounds, cx.handlers, &mut *cx.vm, cx.events);
        }
    }
}

impl DesignSlot {
    /// The frame the element sees while its containers clip it to `clip` (content coordinates): `None` (the real
    /// one) while the pointer is over its visible part — or holds a press that started there, a drag leaving it
    /// like a captured mouse —, else the pointer away, as over another control: the clipped-away part of a
    /// control is not there for the mouse (WinForms).
    fn mask_pointer(&mut self, frame: &kubuno_desktop_controls::host::Frame, clip: Rect) -> Option<kubuno_desktop_controls::host::Frame> {
        let over = !frame.pointer_outside() && clip.contains(frame.mouse.0, frame.mouse.1);
        if !frame.mouse_down {
            self.clip_press = None;
        } else if self.clip_press.is_none() {
            self.clip_press = Some(over);
        }
        if over || self.clip_press == Some(true) {
            return None;
        }
        let away = kubuno_desktop_controls::host::POINTER_AWAY;
        Some(kubuno_desktop_controls::host::Frame { mouse: (away, away), mouse_down: false, right_down: false, middle_down: false, wheel: (0.0, 0.0), ..*frame })
    }

    /// Whether the element publishes a node of the accessibility tree (`offer_services`): an
    /// element with its events and a visual class.
    fn publishes_access(&self) -> bool {
        self.events.is_some() && self.meta.is_some_and(|m| !crate::registry::is_non_visual(m.name))
    }

    /// What the element asks of the window this frame (`crate::common::FrameServices`): its pointer
    /// shape and tooltip when hovered, its node of the accessibility tree, its context menu and
    /// whether files may be dropped on it.
    fn offer_services(&self, cx: &mut PaintCx<'_>, bounds: Rect, enabled: bool, focus_id: Option<kubuno_desktop_ui::FocusId>) {
        let Some(services) = cx.services.as_deref_mut() else { return };
        let Some(slot) = self.events.as_deref() else { return };
        // What its containers' clips leave of it (`crate::clip`): its pointer area, its menu and drop areas, and
        // the bounds assistive technology is given (empty when it is clipped away entirely).
        let client = crate::clip::visible_client(crate::common::to_client(bounds));
        let frame = cx.frame;
        let hovered = !frame.pointer_outside() && client.contains(frame.mouse.0, frame.mouse.1);
        let vm = &*cx.vm;
        if let Some(common) = &self.common {
            if hovered {
                if let Some(cursor) = common.cursor(vm) {
                    services.offer_cursor(cursor);
                }
                if let Some(tip) = common.tooltip(vm) {
                    services.offer_tooltip(tip, client, &slot.id);
                }
            }
            if let Some(menu) = common.context_menu() {
                services.context_menus.push((slot.id.clone(), client, menu.to_string()));
            }
            if common.allow_drop() && enabled {
                services.drop_targets.push((slot.id.clone(), client));
            }
        }
        let Some(meta) = self.meta else { return };
        if crate::registry::is_non_visual(meta.name) {
            return;
        }
        // What the error glyphs need (DATA-2): where the element is and what it is bound to.
        let paths = self.bound_paths.get_or_init(|| {
            slot.attributes
                .iter()
                .filter(|(_, raw)| crate::binding::is_binding_expr(raw))
                .filter_map(|(_, raw)| crate::binding::parse_binding(raw).map(|s| s.path))
                .collect::<Vec<_>>()
                .into()
        });
        if !paths.is_empty() {
            services.bound.push((slot.id.clone(), bounds, client, paths.clone()));
        }
        let attr = |name: &str| -> Option<String> {
            let raw = slot.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())?;
            if crate::binding::is_binding_expr(&raw) {
                let spec = crate::binding::parse_binding(&raw)?;
                // `{Res key}` too: what the element shows is what it is called.
                match crate::resources::get(vm, &spec)? {
                    crate::binding::Value::Str(s) => Some(s),
                    crate::binding::Value::Bool(b) => Some(b.to_string()),
                    crate::binding::Value::F32(v) => Some(v.to_string()),
                    crate::binding::Value::List(_) | crate::binding::Value::Object(_) => None,
                }
            } else {
                Some(raw)
            }
        };
        let (name, description, role) = self.common.as_ref().map(|c| c.accessibility(vm)).unwrap_or((None, None, None));
        let text = ["Text", "Title", "Header", "Label"].iter().find_map(|n| attr(n)).filter(|t| !t.is_empty());
        let (display, key) = text.as_deref().map(crate::common::mnemonic).unwrap_or_default();
        let role = role.and_then(crate::common::role_of).unwrap_or_else(|| crate::common::default_role(meta));
        // A masked single-line field (`PasswordChar`, `UseSystemPasswordChar`): UI Automation sees a
        // password field (`IsPassword`) whose value is the glyphs it shows, never the secret — as
        // assistive technology gets from a Windows password edit.
        let password_glyph = if role == host_access::AccessRole::TextInput && attr("Multiline").as_deref() != Some("true") {
            if attr("UseSystemPasswordChar").as_deref() == Some("true") {
                Some('\u{25CF}')
            } else {
                attr("PasswordChar").and_then(|v| v.chars().next())
            }
        } else {
            None
        };
        let role = if password_glyph.is_some() { host_access::AccessRole::PasswordInput } else { role };
        let input = matches!(
            role,
            host_access::AccessRole::TextInput | host_access::AccessRole::PasswordInput | host_access::AccessRole::MultilineTextInput | host_access::AccessRole::ComboBox
        );
        // The name a screen reader announces: `AccessibleName`, else what the element shows (its
        // text, its placeholder), else its tooltip (an icon button's only words) — the developer's
        // `x:Name` only as a last resort, as Windows Forms falls back to nothing better.
        let tooltip = self.common.as_ref().and_then(|c| c.tooltip(vm));
        let name = name
            .or_else(|| (!input && !display.is_empty()).then(|| display.clone()))
            .or_else(|| attr("Placeholder").filter(|p| !p.is_empty()))
            .or_else(|| ["Description", "DisplayName", "Body"].iter().find_map(|n| attr(n)).filter(|t| !t.is_empty()))
            .or(tooltip)
            .or_else(|| slot.name.clone().filter(|_| !matches!(role, host_access::AccessRole::Group | host_access::AccessRole::Pane | host_access::AccessRole::Separator | host_access::AccessRole::Image)))
            .unwrap_or_default();
        let checked = ["Checked", "On"].iter().find_map(|n| attr(n)).map(|v| v == "true");
        let clickable = meta.is_a("ButtonBase") || slot.native_click || slot.keyboard_click;
        services.access_ids.push((crate::common::access_id(&slot.id), slot.id.clone(), focus_id));
        services.access.push(host_access::AccessNode {
            id: crate::common::access_id(&slot.id),
            // The element painting around this one (its own id is the last of the stack); the id's
            // own parent when nothing is (a tree published outside a frame).
            parent: match services.access_parents.len() {
                n if n >= 2 => Some(services.access_parents[n - 2]),
                _ => parent_id_of(&slot.id).map(|p| crate::common::access_id(&p)),
            },
            role,
            name,
            description: description.unwrap_or_default(),
            value: match password_glyph {
                Some(glyph) => Some(glyph.to_string().repeat(attr("Text").map_or(0, |t| t.chars().count()))),
                None if input => attr("Text").or_else(|| attr("SelectedValue")),
                None => None,
            },
            bounds: (client.left, client.top, client.right, client.bottom),
            focusable: focus_id.is_some() && enabled,
            disabled: !enabled,
            checked: if matches!(role, host_access::AccessRole::CheckBox | host_access::AccessRole::RadioButton | host_access::AccessRole::Switch) {
                Some(checked.unwrap_or(false))
            } else {
                None
            },
            clickable,
            read_only: attr("ReadOnly").is_some_and(|v| v == "true"),
            access_key: key.map(|(k, _)| format!("Alt+{}", k.to_uppercase())),
            expanded: None,
        });
    }
}

use kubuno_desktop_controls::host::access as host_access;

/// Sets the literal properties of `props` on `component` (a named component, configured when the
/// view is built, DATA-2); the bound ones follow at every paint.
pub(crate) fn apply_literal_props(component: &mut dyn crate::component::Component, props: &[CustomProp]) {
    for p in props {
        if let CustomSource::Literal(v) = &p.source {
            component.kubuno_set_property(p.name, v);
        }
    }
}

/// Sets an application class's own properties (EVT-7b) on its instance, from the element's
/// attributes resolved against `vm` — when their value changes only (the first paint, a binding
/// that moved), like the property assignments of a Windows Forms `InitializeComponent`: a value the
/// control changed itself since (a user control's field typed into through its own two-way
/// binding, a `Load` handler's sample data) is not overwritten at the next frame.
pub(crate) fn apply_custom_props(component: &mut dyn crate::component::Component, props: &[CustomProp], vm: &dyn crate::binding::ViewModel) {
    let mut changed = false;
    for p in props {
        if let Some(value) = p.resolve(vm) {
            let mut last = p.last.borrow_mut();
            if last.as_ref() != Some(&value) {
                changed = true;
                *last = Some(value.clone());
                component.kubuno_set_property(p.name, &value);
            }
        }
    }
    // A property that changed (a binding moved) may change the look: the paint buffer is stale
    // (WinForms' property setters invalidate).
    if changed {
        if let Some(control) = component.as_control_mut() {
            control.invalidate();
        }
    }
}

/// Delivers the events `component` queued outside a dispatch (a declared event's `raise_…`,
/// EVT-7b) to its element's handlers — the typed handler or the legacy table entry the element
/// names — and reports each as a [`crate::node::ViewEventKind::Other`]. `slot` is `None` in the
/// designer (the queue is emptied, nothing runs).
#[cfg(test)]
pub(crate) fn deliver_queued(
    component: &mut dyn crate::component::Component,
    slot: Option<&crate::events::router::SlotEvents>,
    bounds: Rect,
    handlers: &mut crate::binding::HandlerTable,
    vm: &mut dyn crate::binding::ViewModel,
    events: &mut Vec<crate::node::ViewEvent>,
) {
    if !component.component_core().has_queued() {
        return;
    }
    let queued = component.component_core_mut().take_queued();
    deliver_taken(queued, slot, bounds, handlers, vm, events);
}

/// [`deliver_queued`] for events already taken from their component.
pub(crate) fn deliver_taken(
    queued: Vec<(&'static str, Box<dyn crate::events::EventArgs>)>,
    slot: Option<&crate::events::router::SlotEvents>,
    bounds: Rect,
    handlers: &mut crate::binding::HandlerTable,
    vm: &mut dyn crate::binding::ViewModel,
    events: &mut Vec<crate::node::ViewEvent>,
) {
    let Some(slot) = slot else { return };
    for (event, mut args) in queued {
        let Some(handler) = slot.handler(event) else { continue };
        let sender = slot.sender(bounds);
        handlers.dispatch_args(handler, &mut *vm, &sender, args.as_mut());
        let args: std::rc::Rc<dyn crate::events::EventArgs> = std::rc::Rc::from(args);
        events.push(crate::node::ViewEvent {
            focus_id: slot.focus_id,
            handler: Some(handler.to_string()),
            kind: crate::node::ViewEventKind::Other { name: event.strip_prefix("On").unwrap_or(event), args },
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Adorner geometry — pure, unit-testable without a `Canvas` (the actual
// painting, in `paint_adorners` below, is what needs the visual check).
// ─────────────────────────────────────────────────────────────────────────

/// The 8 compass-point resize handles for `bounds` — `DESIGNER.md` §1's
/// "resize handles at the 8 compass points" — each a `size`-DIP square
/// centered on its point, in clockwise order starting at North (N, NE, E,
/// SE, S, SW, W, NW), the same order a screen reader / a test asserting on
/// index 0 would expect "top-center" to be.
pub fn resize_handles(bounds: Rect, size: f32) -> [Rect; 8] {
    let half = size / 2.0;
    let cx = (bounds.left + bounds.right) / 2.0;
    let cy = (bounds.top + bounds.bottom) / 2.0;
    let points = [
        (cx, bounds.top),           // N
        (bounds.right, bounds.top), // NE
        (bounds.right, cy),         // E
        (bounds.right, bounds.bottom), // SE
        (cx, bounds.bottom),        // S
        (bounds.left, bounds.bottom), // SW
        (bounds.left, cy),          // W
        (bounds.left, bounds.top),  // NW
    ];
    points.map(|(x, y)| Rect::new(x - half, y - half, x + half, y + half))
}

thread_local! {
    /// See [`set_container_outlines`].
    static CONTAINER_OUTLINES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Turns the dashed outline of the otherwise-invisible containers on or off on this thread's
/// design surface (Visual Studio's "Show design outlines" designer option, sent as the
/// `setDesignOptions` message). On by default, like the dotted border Windows Forms draws around
/// a borderless `Panel`.
pub fn set_container_outlines(on: bool) {
    CONTAINER_OUTLINES.with(|c| c.set(on));
}

/// Whether the design surface outlines its otherwise-invisible containers (see
/// [`set_container_outlines`]).
pub fn container_outlines() -> bool {
    CONTAINER_OUTLINES.with(|c| c.get())
}

/// The dashes of a design-time container outline around `bounds`: 1 DIP thick, 3 DIP of ink every
/// 6, inside the container's own box (so it never spills onto a neighbour).
pub fn container_outline_dashes(bounds: Rect) -> Vec<Rect> {
    dashed_outline(bounds, 3.0, 3.0, 1.0)
}

static DESIGN_TIME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Marks this process as the designer's surface: views compile with their design-time attributes (`d:Text="…"`
/// replaces `Text`, `d:Visible="false"` hides a pane that the data would hide at run time; see
/// `kubuno_desktop_views_meta::inherit::apply_design_attributes`), like XAML's `d:` namespace.
pub fn set_design_time(on: bool) {
    DESIGN_TIME.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether views compile with their design-time attributes (see [`set_design_time`]).
pub fn design_time() -> bool {
    DESIGN_TIME.load(std::sync::atomic::Ordering::Relaxed)
}

/// The id prefix of a control of the base view of an inherited view (`x:Inherited="true"`, see
/// `kubuno_desktop_views_meta::inherit`): painted with a lock, never selectable in the derived view's designer.
pub const INHERITED_LOCKED_PREFIX: &str = "inherited!";

/// The id prefix of the descendants of such a control (`x:Inherited="inner"`): not selectable either.
pub const INHERITED_INNER_PREFIX: &str = "inherited:";

/// The id prefix of the title bar's standard items (`ShowSearch`, `ShowWaffle`… on the view's root,
/// `crate::window::build_header_items`): made by the framework, not elements of the document — never selected; a
/// click on them in the designer selects the view, whose properties show them.
pub const HEADER_ITEM_PREFIX: &str = "header!";

/// The small padlock the designer draws in the top-right corner of an inherited, locked control (the Windows Forms
/// designer's glyph, kept off the text and the value that usually start at the left): a body and a shackle, on a
/// plate so it reads on any control.
pub fn paint_inherited_glyph(c: &dyn kubuno_desktop_ui::Canvas, bounds: Rect) {
    if bounds.right - bounds.left < 14.0 || bounds.bottom - bounds.top < 14.0 {
        return;
    }
    let theme = c.theme();
    let (x, y) = (bounds.right - 14.0, bounds.top + 2.0);
    c.fill_rounded(&Rect::new(x, y, x + 12.0, y + 13.0), 2.0, &theme.window_background);
    c.stroke_rounded_w(&Rect::new(x + 3.5, y + 1.5, x + 8.5, y + 7.5), 2.5, &theme.text_secondary, 1.2);
    c.fill_rounded(&Rect::new(x + 2.0, y + 6.0, x + 10.0, y + 12.0), 1.0, &theme.text_secondary);
}

/// Paints the design-time outline of an otherwise-invisible container: the theme's divider colour
/// (a low-contrast grey in the light and in the dark theme alike), dashed.
pub fn paint_container_outline(c: &dyn kubuno_desktop_ui::Canvas, bounds: Rect) {
    let ink = c.theme().divider;
    for dash in container_outline_dashes(bounds) {
        c.fill_rounded(&dash, 0.0, &ink);
    }
}

/// The small filled rects a "marching ants" / dashed outline paints along
/// `bounds`'s perimeter (`Canvas` has no native dash pattern — see `crate::
/// design`'s own doc for why this crate draws it itself rather than reaching
/// into `kubuno_desktop_controls`), `thickness` DIP thick, alternating `dash` DIP of
/// ink with `gap` DIP of nothing, starting from the top-left corner and
/// going clockwise. Geometry only: what actually gets a colour is
/// `paint_adorners`.
pub fn dashed_outline(bounds: Rect, dash: f32, gap: f32, thickness: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    if dash <= 0.0 || bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        return out;
    }
    let step = dash + gap;

    // Top edge, left → right.
    let mut x = bounds.left;
    while x < bounds.right {
        let end = (x + dash).min(bounds.right);
        out.push(Rect::new(x, bounds.top, end, bounds.top + thickness));
        x += step;
    }
    // Bottom edge, left → right.
    let mut x = bounds.left;
    while x < bounds.right {
        let end = (x + dash).min(bounds.right);
        out.push(Rect::new(x, bounds.bottom - thickness, end, bounds.bottom));
        x += step;
    }
    // Left edge, top → bottom (corners already covered by the horizontal
    // edges above are left slightly overlapping rather than notched out —
    // a dashed rect outline, not a mitred stroke).
    let mut y = bounds.top;
    while y < bounds.bottom {
        let end = (y + dash).min(bounds.bottom);
        out.push(Rect::new(bounds.left, y, bounds.left + thickness, end));
        y += step;
    }
    // Right edge, top → bottom.
    let mut y = bounds.top;
    while y < bounds.bottom {
        let end = (y + dash).min(bounds.bottom);
        out.push(Rect::new(bounds.right - thickness, y, bounds.right, end));
        y += step;
    }

    out
}

/// Paints one frame's adorners over an already-painted view: `parent` (the
/// selected element's own container, dashed outline — `DESIGNER.md` §1's
/// "parent-container dashed outline"), `hover` (a light highlight, skipped
/// when it IS the selection), and `selected` (a solid outline, plus the 8
/// resize handles ONLY when `selected.layout == LayoutKind::DockAnchor` —
/// `DESIGNER.md` §4: only an Anchor/absolute child gets free resize; a flow
/// child gets the plain outline alone). Never draws widget content itself —
/// `crate::runtime::Runtime::frame_with_design` already did that before this
/// runs. Needs a live `Canvas`, so this function itself is exercised by the
/// visual check, not a unit test (`resize_handles`/`dashed_outline` above
/// are the testable geometry this calls into).
///
/// With a multi-selection (`DESIGNER.md` §13) every selected element gets its frame and grab handles,
/// and the **primary** selection (the last clicked, the reference of the Align/Make Same Size
/// commands) is told apart the WinForms way: its handles are white with an accent border, the other
/// selected elements' handles are filled with the accent colour. A grey border instead of an accent
/// one marks an element that cannot be resized here (a flow child).
pub fn paint_adorners(
    c: &dyn kubuno_desktop_controls::ControlCanvas,
    theme: &kubuno_desktop_ui::Theme,
    layout: &LayoutMap,
    selection: &Selection,
    hover: Option<&str>,
) {
    let selected = selection.primary();
    if let Some(sel) = selected.and_then(|id| layout.get(id)) {
        if let Some(parent) = sel.parent_id.as_deref().and_then(|id| layout.get(id)) {
            for dash in dashed_outline(parent.bounds, 4.0, 3.0, 1.0) {
                c.fill_rect(&dash, &theme.text_secondary);
            }
        }
    }

    // Hover: a plain, already-blended token (`accent_light`, the same one
    // every hot-state fill in this codebase already uses — see e.g.
    // `examples/view_embed.rs`'s own "Save"/"Menu" demo buttons) rather than
    // constructing a translucent color by hand — this crate has no
    // dependency on the raw D2D color type to build one from (`kubuno_desktop_ui`
    // only ever hands out already-resolved theme colors, by design).
    if let Some(h) = hover.filter(|id| !selection.contains(id)).and_then(|id| layout.get(id)) {
        c.stroke_rounded_w(&h.bounds, 0.0, &theme.accent_light, 1.0);
    }

    // The root element is the view itself: its selection is drawn by the view frame
    // (`paint_view_frame`), whose own resize handles replace the element grab handles. The primary
    // is painted last, on top of the others.
    let ordered = selection.ids().iter().filter(|id| Some(id.as_str()) != selected).chain(selection.ids().iter().filter(|id| Some(id.as_str()) == selected));
    for sel in ordered.filter(|id| !id.is_empty()).filter_map(|id| layout.get(id)) {
        let primary = Some(sel.id.as_str()) == selected;
        // Drawn 3 DIP OUTSIDE the element so it stays visible on an accent-coloured control (a primary
        // Button was invisible under an outline hugging its own blue edge - found in a visual check).
        let outline = selection_frame(sel.bounds);
        let resizable = sel.layout == LayoutKind::DockAnchor;
        let ink = if resizable { &theme.accent } else { &theme.text_secondary };
        c.stroke_rounded_w(&outline, 0.0, ink, 1.0);
        // WinForms-style grab handles on every selected element: white with a border on the primary
        // selection, filled on the others; an accent border when the element can be resized here (an
        // Anchor child - the only case `handle_at` hit-tests), grey otherwise.
        for handle in resize_handles(outline, RESIZE_HANDLE_SIZE) {
            if primary {
                c.fill_rect(&handle, &theme.card_background);
                c.stroke_rect(&handle, ink);
            } else {
                c.fill_rect(&handle, ink);
                c.stroke_rect(&handle, &theme.card_background);
            }
        }
    }
}

/// The 1-device-pixel dot squares a WinForms-style marquee ("marching ants")
/// paints along `bounds`'s perimeter, alternating 1 device pixel of ink with 1
/// device pixel of nothing. Unlike `dashed_outline` (DIP-sized dashes, fine
/// for the parent-container outline), the rubber-band selection must read as
/// crisp, round-free DOTS at any DPI — the WinForms designer reference. DIP
/// dashes rounded per rect can't guarantee an exact "1 px on / 1 px off", so
/// this walks the perimeter directly in DEVICE pixels (`scale` is
/// `Canvas::scale()`, DPI / 96) and only converts back to DIP for the
/// resulting `Rect`s — each dot then lands exactly on the device pixel grid
/// `ControlCanvas::fill_rect` itself snaps to, with no drift, at 175% DPI or
/// any other. Geometry only, unit-testable with no live `Canvas` (`paint_marquee`
/// below is what colours it in).
pub fn dotted_outline(bounds: Rect, scale: f32) -> Vec<Rect> {
    let scale = scale.max(0.01);
    let px = |v: f32| (v * scale).round() as i64;
    let dip = |v: i64| v as f32 / scale;

    let left = px(bounds.left);
    let top = px(bounds.top);
    let right = px(bounds.right);
    let bottom = px(bounds.bottom);
    let mut out = Vec::new();
    if right <= left || bottom <= top {
        return out;
    }

    // Top edge, left → right, one device pixel of ink every two, starting at
    // the corner.
    let mut x = left;
    while x < right {
        out.push(Rect::new(dip(x), dip(top), dip(x + 1), dip(top + 1)));
        x += 2;
    }
    // Bottom edge, left → right.
    let mut x = left;
    while x < right {
        out.push(Rect::new(dip(x), dip(bottom - 1), dip(x + 1), dip(bottom)));
        x += 2;
    }
    // Left edge, top → bottom (corners already covered by the horizontal
    // edges above are left slightly overlapping, like `dashed_outline`).
    let mut y = top;
    while y < bottom {
        out.push(Rect::new(dip(left), dip(y), dip(left + 1), dip(y + 1)));
        y += 2;
    }
    // Right edge, top → bottom.
    let mut y = top;
    while y < bottom {
        out.push(Rect::new(dip(right - 1), dip(y), dip(right), dip(y + 1)));
        y += 2;
    }

    out
}

/// Paints the marquee (rubber-band) rectangle of an in-progress marquee selection: a thin light line
/// under accent DOTS (WinForms rubber-band style, `dotted_outline`), so it reads on the light view and
/// on the dark canvas alike.
pub fn paint_marquee(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &kubuno_desktop_ui::Theme, rect: Rect) {
    c.stroke_rounded_w(&rect, 0.0, &theme.card_background, 1.0);
    for dot in dotted_outline(rect, c.scale()) {
        c.fill_rect(&dot, &theme.accent);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// DSG-9: move/resize drag geometry (Anchor), reorder geometry (Flow),
// snapping — pure `Rect` arithmetic, unit-testable with no `Canvas` (the
// actual ghost/marker painting, further below, is what needs the visual
// check — same split as the adorner geometry above).
// ─────────────────────────────────────────────────────────────────────────

/// The selection frame around an element's `bounds`: the outline and the grab handles are drawn on
/// it, and [`handle_at`] is hit-tested against it ([`SELECTION_FRAME_GAP`] DIP outside the element, so
/// the frame stays visible on an accent-coloured control).
pub fn selection_frame(bounds: Rect) -> Rect {
    Rect::new(
        bounds.left - SELECTION_FRAME_GAP,
        bounds.top - SELECTION_FRAME_GAP,
        bounds.right + SELECTION_FRAME_GAP,
        bounds.bottom + SELECTION_FRAME_GAP,
    )
}

/// See [`selection_frame`].
pub const SELECTION_FRAME_GAP: f32 = 3.0;

/// The size (DIP) [`resize_handles`] paints its 8 compass squares at, and
/// what [`handle_at`] hit-tests against — one constant so painting and
/// hit-testing can never silently drift apart.
pub const RESIZE_HANDLE_SIZE: f32 = 8.0;

/// A minimum size a resize never shrinks below, DIP — avoids a degenerate
/// (zero/negative) `Width`/`Height` that [`LayoutMap::hit_test`] would then
/// have to skip as invisible (see that method's own doc), and a `Width`/
/// `Height` an author would have to type over before the element is usable
/// again.
pub const MIN_ELEMENT_SIZE: f32 = 4.0;

/// The snap threshold, DIP — `DESIGNER.md` §1's "snaplines … with a small
/// threshold". WinForms' own designer default is in the same neighbourhood.
pub const SNAP_THRESHOLD: f32 = 6.0;

/// A resize handle, in [`resize_handles`]'s own clockwise-from-N order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Handle {
    /// The exact order [`resize_handles`] returns its 8 squares in.
    const ORDER: [Handle; 8] =
        [Handle::N, Handle::NE, Handle::E, Handle::SE, Handle::S, Handle::SW, Handle::W, Handle::NW];
}

/// Which resize handle of `bounds` (if any) contains `(x, y)` — the hit-test
/// half of [`resize_handles`]'s geometry, used both to decide whether a press
/// arms a resize drag ([`DesignController::press`]) and, by a caller that
/// wants it, to choose a resize cursor.
pub fn handle_at(bounds: Rect, handle_size: f32, x: f32, y: f32) -> Option<Handle> {
    let handles = resize_handles(bounds, handle_size);
    handles.iter().zip(Handle::ORDER).find(|(r, _)| r.contains(x, y)).map(|(_, h)| h)
}

/// `start` translated by `(dx, dy)`, size unchanged — the geometry of an
/// Anchor/absolute move drag (`DESIGNER.md` §4's "Free drag anywhere").
pub fn move_rect(start: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(start.left + dx, start.top + dy, start.right + dx, start.bottom + dy)
}

/// `start` resized by dragging `handle` by `(dx, dy)`: the edge(s) opposite
/// the dragged handle never move, the dragged edge(s) follow the pointer
/// exactly until the clamp at [`MIN_ELEMENT_SIZE`] would invert the box (a
/// clamp that also implicitly caps how far a `W`/`N`/`NW`/`SW`/`NE` handle's
/// leading edge can travel, so `left`/`top` never cross `right`/`bottom`).
/// `DESIGNER.md` §4's "resize handles change `Width`/`Height` and, per the
/// `Anchor` edges set, may also change `X`/`Y`" — this function does not
/// itself read `Anchor` (a resize is always computed against all four
/// numbers; which of them the caller actually WRITES back is decided
/// separately, by comparing the result to `start` — see
/// [`DesignController::end_drag`]).
pub fn resize_rect(start: Rect, handle: Handle, dx: f32, dy: f32) -> Rect {
    let mut left = start.left;
    let mut top = start.top;
    let mut right = start.right;
    let mut bottom = start.bottom;

    if matches!(handle, Handle::NE | Handle::E | Handle::SE) {
        right = (start.right + dx).max(start.left + MIN_ELEMENT_SIZE);
    }
    if matches!(handle, Handle::NW | Handle::W | Handle::SW) {
        left = (start.left + dx).min(start.right - MIN_ELEMENT_SIZE);
    }
    if matches!(handle, Handle::SW | Handle::S | Handle::SE) {
        bottom = (start.bottom + dy).max(start.top + MIN_ELEMENT_SIZE);
    }
    if matches!(handle, Handle::NW | Handle::N | Handle::NE) {
        top = (start.top + dy).min(start.bottom - MIN_ELEMENT_SIZE);
    }

    Rect::new(left, top, right, bottom)
}

/// Which axis a snapline runs along — `Vertical` (a line at a constant X,
/// drawn top-to-bottom) fires from an X-coordinate match (left/right/centre
/// edges), `Horizontal` (a line at a constant Y) from a Y match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapAxis {
    Vertical,
    Horizontal,
}

/// One snapline that fired — `at` is the exact coordinate the dragged edge
/// snapped to (what a caller paints the guide line at), `axis` says which
/// coordinate it constrains. `DESIGNER.md` §1's "blue guide lines when an
/// edge or center aligns with a sibling's".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapGuide {
    pub axis: SnapAxis,
    pub at: f32,
}

/// Snaps `bounds`' left/right/h-centre independently from its top/bottom/
/// v-centre against every line `candidates` offers (each candidate rect
/// contributes its own left/right/centre-x and top/bottom/centre-y), within
/// `threshold` DIP, picking the CLOSEST match per axis when more than one is
/// in range. `candidates` is deliberately just "every other bounds worth
/// snapping to" (siblings' own painted rects, the container's own bounds) —
/// `LayoutMap` records painted bounds only, no padding value (see that
/// struct's own doc), so "container padding" (`DESIGNER.md` DSG-9 item 1)
/// snaps to the container's own outer edge, the padding-`0` line; a caller
/// that has a real padding number could pass an already-inset container rect
/// instead, this function does not care where a candidate rect came from.
/// Returns the (possibly shifted, same size) `bounds` and the guides that
/// fired, for a caller to paint — an empty `Vec` when nothing was within
/// `threshold` on either axis.
pub fn snap_bounds(bounds: Rect, candidates: &[Rect], threshold: f32) -> (Rect, Vec<SnapGuide>) {
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;

    let x_lines = [bounds.left, bounds.right, (bounds.left + bounds.right) / 2.0];
    let y_lines = [bounds.top, bounds.bottom, (bounds.top + bounds.bottom) / 2.0];
    let x_candidates = candidates.iter().flat_map(|c| [c.left, c.right, (c.left + c.right) / 2.0]);
    let y_candidates = candidates.iter().flat_map(|c| [c.top, c.bottom, (c.top + c.bottom) / 2.0]);

    let (dx, guide_x) = best_snap_delta(&x_lines, x_candidates, threshold);
    let (dy, guide_y) = best_snap_delta(&y_lines, y_candidates, threshold);

    let snapped = Rect::new(bounds.left + dx, bounds.top + dy, bounds.left + dx + width, bounds.top + dy + height);
    let mut guides = Vec::new();
    if let Some(at) = guide_x {
        guides.push(SnapGuide { axis: SnapAxis::Vertical, at });
    }
    if let Some(at) = guide_y {
        guides.push(SnapGuide { axis: SnapAxis::Horizontal, at });
    }
    (snapped, guides)
}

/// The smallest delta that moves any of `source_lines` onto any of
/// `candidate_lines`, within `threshold` — `(0.0, None)` when nothing is
/// close enough. Shared by [`snap_bounds`]'s X and Y passes.
fn best_snap_delta(source_lines: &[f32; 3], candidate_lines: impl Iterator<Item = f32>, threshold: f32) -> (f32, Option<f32>) {
    let mut best: Option<(f32, f32, f32)> = None; // (distance, delta, target)
    for cand in candidate_lines {
        for &line in source_lines {
            let d = (cand - line).abs();
            if d <= threshold && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, cand - line, cand));
            }
        }
    }
    match best {
        Some((_, delta, target)) => (delta, Some(target)),
        None => (0.0, None),
    }
}

/// Which way `siblings` are laid out, inferred from which axis their centres
/// actually spread across — `LayoutMap` does not carry the container's own
/// `Direction` attribute (see [`snap_bounds`]'s doc for the same kind of
/// approximation), so comparing the spread is a robust proxy that needs no
/// extra wire field: a `TopDown`/`BottomUp` stack's children vary mostly in Y,
/// a `LeftToRight`/`RightToLeft` one mostly in X. Defaults to `Vertical`
/// (`TopDown`, `crate::registry::families::containers`'s own default flow
/// direction) when there is not enough information (0 or 1 siblings, or an
/// exact tie) to tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAxis {
    Horizontal,
    Vertical,
}

pub fn flow_axis(siblings: &[Rect]) -> FlowAxis {
    if siblings.len() < 2 {
        return FlowAxis::Vertical;
    }
    let xs: Vec<f32> = siblings.iter().map(|r| (r.left + r.right) / 2.0).collect();
    let ys: Vec<f32> = siblings.iter().map(|r| (r.top + r.bottom) / 2.0).collect();
    let spread = |v: &[f32]| {
        let min = v.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        max - min
    };
    if spread(&xs) > spread(&ys) {
        FlowAxis::Horizontal
    } else {
        FlowAxis::Vertical
    }
}

/// The 0-based index (`Vec::insert` semantics, matching `kubuno_desktop_views::edit::
/// move_child`/`move_element`) a Flow drop/reorder at `(x, y)` would land at,
/// given `siblings` in document order — `DESIGNER.md` DSG-9 item 2's
/// "insertion marker between siblings". Compares the point's coordinate along
/// [`flow_axis`] against each sibling's own midpoint on that axis; the
/// returned index is the count of siblings whose midpoint comes before the
/// point (an empty `siblings` always returns `0`).
pub fn flow_insertion_index(siblings: &[Rect], x: f32, y: f32) -> usize {
    let axis = flow_axis(siblings);
    let point = match axis {
        FlowAxis::Horizontal => x,
        FlowAxis::Vertical => y,
    };
    siblings
        .iter()
        .filter(|r| {
            let mid = match axis {
                FlowAxis::Horizontal => (r.left + r.right) / 2.0,
                FlowAxis::Vertical => (r.top + r.bottom) / 2.0,
            };
            mid < point
        })
        .count()
}

/// A thin marker rect between `siblings[index - 1]` and `siblings[index]`
/// (or before the first / after the last), spanning `container`'s
/// cross-axis extent, `thickness` DIP thick — the paintable geometry for
/// `DESIGNER.md` DSG-9 item 2's "insertion marker" / item 3's Flow drop
/// marker.
pub fn flow_insertion_marker(siblings: &[Rect], container: Rect, index: usize, thickness: f32) -> Rect {
    let axis = flow_axis(siblings);
    let half = thickness / 2.0;
    let boundary = match axis {
        FlowAxis::Horizontal => {
            if siblings.is_empty() {
                container.left
            } else if index == 0 {
                siblings[0].left
            } else if index >= siblings.len() {
                siblings[siblings.len() - 1].right
            } else {
                (siblings[index - 1].right + siblings[index].left) / 2.0
            }
        }
        FlowAxis::Vertical => {
            if siblings.is_empty() {
                container.top
            } else if index == 0 {
                siblings[0].top
            } else if index >= siblings.len() {
                siblings[siblings.len() - 1].bottom
            } else {
                (siblings[index - 1].bottom + siblings[index].top) / 2.0
            }
        }
    };
    match axis {
        FlowAxis::Horizontal => Rect::new(boundary - half, container.top, boundary + half, container.bottom),
        FlowAxis::Vertical => Rect::new(container.left, boundary - half, container.right, boundary + half),
    }
}

/// Paints a drag's live ghost outline (`preview`) plus any active
/// [`SnapGuide`]s spanning `canvas_bounds` — the local-only visual feedback
/// `DESIGNER.md` §2 requires ("intermediate mouse-move frames update the
/// design surface's local, client-side preview only … never touching
/// text"). Needs a live `Canvas`, so exercised by the visual check, not a
/// unit test — [`snap_bounds`]/[`resize_rect`]/[`move_rect`] above are the
/// testable geometry this reads.
pub fn paint_drag_preview(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &kubuno_desktop_ui::Theme, preview: Rect, guides: &[SnapGuide], canvas_bounds: Rect) {
    c.stroke_rounded_w(&preview, 0.0, &theme.accent, 2.0);
    for guide in guides {
        let line = match guide.axis {
            SnapAxis::Vertical => Rect::new(guide.at - 0.5, canvas_bounds.top, guide.at + 0.5, canvas_bounds.bottom),
            SnapAxis::Horizontal => Rect::new(canvas_bounds.left, guide.at - 0.5, canvas_bounds.right, guide.at + 0.5),
        };
        c.fill_rect(&line, &theme.accent);
    }
}

/// Paints a Flow reorder/drop insertion marker — see
/// [`paint_drag_preview`]'s own doc on why this needs a live `Canvas`.
pub fn paint_insertion_marker(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &kubuno_desktop_ui::Theme, marker: Rect) {
    c.fill_rect(&marker, &theme.accent);
}

/// Paints a toolbox [`DropTarget`]'s marker: an outline ghost at
/// `target.xy`'s placement size for an Anchor drop, a filled insertion line
/// for a Flow drop — `theme.danger` instead of `theme.accent` when
/// `target.valid` is `false` (`DESIGNER.md` DSG-9 item 3's "not allowed …
/// marker when invalid").
pub fn paint_drop_marker(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &kubuno_desktop_ui::Theme, target: &DropTarget) {
    let color = if target.valid { &theme.accent } else { &theme.danger };
    if target.xy.is_some() {
        c.stroke_rounded_w(&target.marker, 0.0, color, 2.0);
    } else {
        c.fill_rect(&target.marker, color);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Request generation — pure, driven by a small host-agnostic input struct so
// it is unit-testable without `kubuno_desktop_controls::host`'s global key state
// (see the module doc).
// ─────────────────────────────────────────────────────────────────────────

/// One `kubuno/applyEdit` op (`DESIGNER.md` §8), the exact shape DSG-2
/// already fixed — this module never invents its own edit vocabulary, only
/// ever produces one of these ops (`setAttribute` for nudge/move/resize,
/// `removeElement` for Delete, `moveElement` for a DSG-9 flow reorder,
/// `insertChild` for a DSG-9 toolbox drop), serialised by `crate::protocol`
/// with the identical field names DSG-2's `kubuno/applyEdit` documents. Every
/// multi-word field name gets an explicit `#[serde(rename = "...")]` rather
/// than relying on the enum's own `rename_all = "camelCase"`, which — per
/// `DESIGNER.md` §9's own note, confirmed live during this crate's
/// development — only camelCases the variant's TAG, never a struct variant's
/// own field names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditOp {
    SetAttribute {
        #[serde(rename = "elementId")]
        element_id: String,
        name: String,
        value: String,
    },
    RemoveElement {
        #[serde(rename = "elementId")]
        element_id: String,
    },
    /// A DSG-9 flow-container reorder (§4's "Flow … drop shows an insertion
    /// caret … drop → `MoveChild`/`MoveElement`"). `new_parent_id` is the
    /// SAME id as the element's current parent for an in-place reorder — this
    /// module never produces a cross-container move (out of DSG-9's own
    /// scope, §6's table); the field exists because it is what DSG-2's
    /// `moveElement` op already carries (`DESIGNER.md` §8).
    MoveElement {
        #[serde(rename = "elementId")]
        element_id: String,
        #[serde(rename = "newParentId")]
        new_parent_id: String,
        index: usize,
    },
    /// A DSG-9 toolbox drop (§4/§6's "on drop emits `insertChild`"). `xml` is
    /// a well-formed `.kbview` fragment — the SAME field name DSG-2's own
    /// `insertChild` op already uses (`DESIGNER.md` §8's "`insertChild
    /// {parentId, index, xml}`"), not the informal "`element`" the work
    /// package's own brief used in prose: the host forwards this op to
    /// `kubuno-desktop-views-ls`'s `kubuno/applyEdit` byte-for-byte, so it must match
    /// that method's already-shipped wire shape exactly.
    InsertChild {
        #[serde(rename = "parentId")]
        parent_id: String,
        index: usize,
        xml: String,
    },
}

/// Which kind of gesture a batched [`SurfaceMessage::EditRequests`]
/// (`crate::protocol`) carries — DSG-9's protocol extension (`DESIGNER.md`
/// §9): `{"ops":[...],"gesture":"move"|"resize"}`, so the host applies every
/// op in one `IOleUndoManager`/`ITextUndoHistory` compound action (DSG-5's
/// own scope, §2: "the same granularity WinForms' own designer gives a
/// drag").
///
/// `DESIGNER.md` §13 adds `"delete"` (Delete on a multi-selection: one `removeElement` per element)
/// and `"format"` (an Align / Make Same Size / Spacing / Center command, [`format_ops`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gesture {
    Move,
    Resize,
    Delete,
    Format,
}

/// One frame's raw key state relevant to design mode — computed by the
/// caller from whatever input source it has (`kubuno_desktop_controls::host`'s
/// global reader in `examples/view_embed.rs`, a plain struct literal in this
/// module's own tests), so [`DesignController::handle_keys`] itself needs no
/// live window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DesignKeyInput {
    pub escape: bool,
    pub delete: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub shift: bool,
    /// Ctrl+A: select every sibling of the primary selection (every child of the view's root when
    /// the view itself, or nothing, is selected).
    pub select_all: bool,
}

/// The modifier keys held during a press on the surface: Ctrl toggles the element under the pointer
/// in the selection, Shift adds it (WinForms), and either one turns a marquee into a toggle/add.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointerModifiers {
    pub ctrl: bool,
    pub shift: bool,
}

/// The designer's selection (`DESIGNER.md` §13): an ordered set of stable element ids plus the
/// **primary** selection - the last clicked one, drawn with white grab handles, the reference of the
/// Align/Make Same Size commands and the element the XML pane and the Properties window's combo show.
/// The root element (`""`, the view itself) is never part of a multi-selection: selecting it alone
/// replaces everything, adding anything to it replaces it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    ids: Vec<String>,
    primary: Option<String>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    /// A selection of `id` alone.
    pub fn single(id: impl Into<String>) -> Self {
        let id = id.into();
        Self { ids: vec![id.clone()], primary: Some(id) }
    }

    pub fn primary(&self) -> Option<&str> {
        self.primary.as_deref()
    }

    /// Every selected id, in selection order.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|s| s == id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.primary = None;
    }

    /// `id` alone (a plain click, a host `select`), or nothing for `None`.
    pub fn set_single(&mut self, id: Option<String>) {
        match id {
            Some(id) => *self = Self::single(id),
            None => self.clear(),
        }
    }

    /// Replaces the selection with `ids` (duplicates dropped, the root dropped when anything else is
    /// there); `primary` when it is one of them, else the first.
    pub fn set_many(&mut self, ids: Vec<String>, primary: Option<String>) {
        let mut unique: Vec<String> = Vec::new();
        for id in ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        if unique.len() > 1 {
            unique.retain(|id| !id.is_empty());
        }
        let primary = primary.filter(|p| unique.contains(p)).or_else(|| unique.first().cloned());
        self.ids = unique;
        self.primary = primary;
    }

    /// Makes `id` the primary selection when it is selected; false otherwise.
    pub fn make_primary(&mut self, id: &str) -> bool {
        if self.contains(id) {
            self.primary = Some(id.to_string());
            true
        } else {
            false
        }
    }

    /// Shift+click: adds `id` (the new primary). The root replaces the selection, and is replaced.
    pub fn add(&mut self, id: String) {
        if id.is_empty() || self.ids.iter().all(|s| s.is_empty()) {
            *self = Self::single(id);
            return;
        }
        if !self.contains(&id) {
            self.ids.push(id.clone());
        }
        self.primary = Some(id);
    }

    /// Ctrl+click: removes `id` when selected (the primary moves to the last one left), adds it as the
    /// new primary otherwise.
    pub fn toggle(&mut self, id: String) {
        if self.contains(&id) {
            self.ids.retain(|s| *s != id);
            if self.primary.as_deref() == Some(id.as_str()) {
                self.primary = self.ids.last().cloned();
            }
        } else {
            self.add(id);
        }
    }
}

// `is_ancestor_id` and `top_level_ids` (the id arithmetic of a multi-selection) live in the
// platform-neutral `kubuno-desktop-views-syntax` (`ids`, WV-1), re-exported here under their historical paths.
pub use kubuno_desktop_views_syntax::ids::{is_ancestor_id, top_level_ids};

/// Selection/hover state plus the request-generating logic — `DESIGNER.md`
/// §6's DSG-6 scope item 2: "a click selects the element under the cursor …
/// Esc selects parent, Delete requests removal, arrows nudge (absolute) —
/// all emitted as requests, never applied locally to the text". Nothing here
/// ever mutates `.kbview` text itself; every state-changing gesture becomes
/// an [`EditOp`] the caller forwards to the host (`crate::protocol::
/// SurfaceMessage::EditRequest`), which forwards it to `kubuno-desktop-views-ls`'s
/// `kubuno/applyEdit`.
///
/// The selection is a [`Selection`] (`DESIGNER.md` §13): Ctrl+click toggles, Shift+click adds, a
/// press on the empty area of a container starts a marquee (rubber band) selecting the children of
/// that container it touches, Ctrl+A selects every sibling; a move/resize drag, an arrow nudge and
/// Delete apply to every selected element.
// No `Debug` derive: `DragSession` embeds `Rect`, which does not implement
// it (see that struct's own comment) — `DesignController` never needed
// `{:?}` printing itself (only its own `EditOp`/`Gesture` results do, and
// those derive `Debug` independently).
#[derive(Default)]
pub struct DesignController {
    enabled: bool,
    selection: Selection,
    hover: Option<String>,
    /// DSG-9's move/resize/reorder drag state, `None` between gestures — see
    /// [`Self::press`]/[`Self::update_drag`]/[`Self::end_drag`]/
    /// [`Self::cancel_drag`].
    drag: Option<DragSession>,
    /// An in-progress marquee selection (`DESIGNER.md` §13), `None` otherwise.
    marquee: Option<MarqueeSession>,
}

/// Which gesture a [`DragSession`] is — the drag's OWN kind, decided once at
/// [`DesignController::press`] time from the selected element's recorded
/// [`LayoutKind`] (`DockAnchor` → `Move`/`Resize`, `Flow` → `Reorder`) and
/// never changes for the life of the session.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragKind {
    Move,
    Resize(Handle),
    Reorder,
}

/// One in-progress drag — local state only, never written to `.kbview` text
/// until [`DesignController::end_drag`] (`DESIGNER.md` §2's "intermediate
/// mouse-move frames update the design surface's local, client-side preview
/// only").
// No `Debug` derive: `Rect` (from `kubuno_drive_desktop_app_controls`, re-exported by
// `kubuno_desktop_ui`) does not implement it — see [`LayoutEntry`]'s own struct for
// the identical constraint.
#[derive(Clone)]
struct DragSession {
    kind: DragKind,
    /// The primary element (the one pressed, or whose handle was grabbed).
    element_id: String,
    parent_id: Option<String>,
    start_mouse: (f32, f32),
    /// The primary element's own painted bounds when the drag armed — used as
    /// the base every subsequent [`DesignController::update_drag`] offsets
    /// from. Absolute canvas DIP, same space as every [`LayoutEntry::bounds`]
    /// (a pure translation of the `X`/`Y`/`Width`/`Height` attribute space,
    /// so an absolute delta equals the local one).
    start_bounds: Rect,
    /// This frame's live, post-snap bounds of the primary element (`Move`/`Resize` only).
    live_bounds: Rect,
    /// `Move`/`Resize`: every element the gesture applies to (the top-level selected Anchor
    /// children, the primary included) with its bounds when the drag armed, and its live bounds.
    members: Vec<(String, Rect, Rect)>,
    /// The snaplines that fired producing `live_bounds` — what
    /// [`DesignController::drag_guides`] paints.
    guides: Vec<SnapGuide>,
    /// `Reorder` only: the insertion index [`flow_insertion_index`] computed
    /// against the OTHER children of `parent_id` (the dragged element itself
    /// excluded) at the current pointer position.
    reorder_index: usize,
    /// `Reorder` only: `element_id`'s own index in `parent_id` when the drag
    /// armed — [`DesignController::end_drag`] is a no-op when the pointer
    /// never actually moved the index anywhere.
    reorder_original_index: usize,
    /// `Reorder` of a menu row only: another level of its menu it goes to (`crate::menus::drop_target`):
    /// the parent, the index among its element children, the insertion marker.
    menu_target: Option<(String, usize, Rect)>,
    /// Whether the pointer has moved past [`DRAG_THRESHOLD`] from
    /// `start_mouse` yet. `false` for a session armed by a plain body-press
    /// (select-and-arm in one gesture); `true` from the start for a
    /// resize-handle press (unambiguous intent). Before that, a session
    /// exists (so mouse-up still reaches [`DesignController::end_drag`] and
    /// cleans it up) but produces no visible preview and no `EditOp` —
    /// indistinguishable from a plain click that merely selected.
    confirmed: bool,
}

/// How a marquee combines with the selection it started from: a plain drag replaces it, Ctrl
/// toggles every element the rectangle touches, Shift adds them (WinForms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarqueeMode {
    Replace,
    Toggle,
    Add,
}

/// An in-progress marquee selection inside `container_id` (the container whose empty area was
/// pressed): nothing is selected until the release.
#[derive(Clone)]
struct MarqueeSession {
    container_id: String,
    start: (f32, f32),
    current: (f32, f32),
    mode: MarqueeMode,
    /// Past [`DRAG_THRESHOLD`]: a plain click on a container's empty area is not a marquee.
    confirmed: bool,
    /// The container's children the rectangle touches right now.
    hits: Vec<String>,
}

/// How far the pointer must move from the press point before an ARMED
/// (`confirmed: false`) drag actually starts moving/reordering the element —
/// `DESIGNER.md` DSG-9 item 1: "start the drag once the pointer passes the
/// drag threshold", the same WinForms/XAML-designer distinction between "a
/// click that happens to land on a control" and "a drag of that control".
pub const DRAG_THRESHOLD: f32 = 4.0;

/// The normalized rectangle between two pointer positions (a marquee's).
pub fn marquee_rect(a: (f32, f32), b: (f32, f32)) -> Rect {
    Rect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
}

/// The direct children of `container_id` a marquee `rect` touches (WinForms: every control the
/// rectangle intersects, within the container where the drag started), in paint (document) order.
/// Degenerate (zero-area) children are skipped, like [`LayoutMap::hit_test`] does.
pub fn marquee_hits(layout: &LayoutMap, container_id: &str, rect: Rect) -> Vec<String> {
    layout
        .entries()
        .iter()
        .filter(|e| e.parent_id.as_deref() == Some(container_id))
        .filter(|e| e.bounds.right > e.bounds.left && e.bounds.bottom > e.bounds.top)
        .filter(|e| e.bounds.left <= rect.right && e.bounds.right >= rect.left && e.bounds.top <= rect.bottom && e.bounds.bottom >= rect.top)
        .map(|e| e.id.clone())
        .collect()
}

/// The smallest rect containing every rect of `rects` (`None` for none).
pub fn union_rect(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().fold(None, |acc: Option<Rect>, r| {
        Some(match acc {
            None => r,
            Some(a) => Rect::new(a.left.min(r.left), a.top.min(r.top), a.right.max(r.right), a.bottom.max(r.bottom)),
        })
    })
}

/// `start` with its four edges moved by `deltas` (left, top, right, bottom) - how a group resize
/// applies the primary element's own edge movement to every other selected element (WinForms resizes
/// every selected control by the same amount). Never smaller than [`MIN_ELEMENT_SIZE`]: the moving
/// edge stops, the other one stays put.
pub fn apply_edge_deltas(start: Rect, deltas: (f32, f32, f32, f32)) -> Rect {
    let (dl, dt, dr, db) = deltas;
    let (mut left, mut top, mut right, mut bottom) = (start.left + dl, start.top + dt, start.right + dr, start.bottom + db);
    if right - left < MIN_ELEMENT_SIZE {
        if dl != 0.0 {
            left = right - MIN_ELEMENT_SIZE;
        } else {
            right = left + MIN_ELEMENT_SIZE;
        }
    }
    if bottom - top < MIN_ELEMENT_SIZE {
        if dt != 0.0 {
            top = bottom - MIN_ELEMENT_SIZE;
        } else {
            bottom = top + MIN_ELEMENT_SIZE;
        }
    }
    Rect::new(left, top, right, bottom)
}

impl DesignController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// `kubuno/setDesignMode {on}` (host → surface). Turning it off clears
    /// hover (a stale highlight from a moment ago would be misleading) but
    /// deliberately keeps the selection — flipping back on with the same
    /// element still selected is the more useful default, and the host is
    /// free to send an explicit `select {id: null}` first if it wants the
    /// opposite.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.hover = None;
        }
    }

    /// The PRIMARY selection (the whole selection is [`Self::selection`]).
    pub fn selected(&self) -> Option<&str> {
        self.selection.primary()
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn hover(&self) -> Option<&str> {
        self.hover.as_deref()
    }

    /// `kubuno/select {id}` (host → surface) — e.g. the XML pane's caret
    /// moved onto a different element (`DESIGNER.md` §1's XML → Design
    /// selection sync). Replaces the whole selection; `None` clears it.
    pub fn set_selected(&mut self, id: Option<String>) {
        self.selection.set_single(id);
    }

    /// A right-click on `id`: it becomes the primary selection when it is already selected (the
    /// context menu then applies to the whole multi-selection, like WinForms), else it is selected alone.
    pub fn select_for_context(&mut self, id: String) {
        if !self.selection.make_primary(&id) {
            self.selection.set_single(Some(id));
        }
    }

    /// `kubuno/selectMany {ids, primary}` (host → surface, `DESIGNER.md` §13).
    pub fn set_selection(&mut self, ids: Vec<String>, primary: Option<String>) {
        self.selection.set_many(ids, primary);
    }

    /// Updates the hovered element from a raw pointer position — a no-op
    /// while design mode is off, so a caller can call this unconditionally
    /// every frame without its own `if design_mode` guard.
    pub fn update_hover(&mut self, layout: &LayoutMap, x: f32, y: f32) {
        if !self.enabled {
            return;
        }
        self.hover = layout.hit_test(x, y).map(|e| e.id.clone());
    }

    /// A completed click (press-then-release over the surface, the same
    /// `press_release` shape `crate::node` already uses for a widget) at
    /// `x, y`: selects the deepest element there, or clears the selection
    /// when the click landed on nothing. Returns whether the selection
    /// actually changed (what the caller uses to decide whether to emit
    /// `selectionChanged`) — a no-op, returning `false`, while design mode
    /// is off.
    pub fn click_select(&mut self, layout: &LayoutMap, x: f32, y: f32) -> bool {
        if !self.enabled {
            return false;
        }
        let hit = layout.hit_test(x, y).map(|e| e.id.clone());
        let changed = hit.as_deref() != self.selection.primary() || self.selection.len() > 1;
        self.selection.set_single(hit);
        changed
    }

    /// Esc/Delete/arrow/Ctrl+A handling for the current selection — see the struct
    /// doc. Esc cancels a drag or a marquee, else moves the selection to the primary's parent (no
    /// [`EditOp`]); Ctrl+A selects the primary's siblings; Delete produces one `removeElement` per
    /// top-level selected element ([`top_level_ids`]); a nudging arrow one `setAttribute` per axis
    /// and per selected Anchor child. `doc` is the CURRENT document's parsed AST, needed only for a
    /// nudge (to read each element's current literal `X`/`Y` before offsetting it — an `EditOp`
    /// carries the new ABSOLUTE value, never a delta, per DSG-2's own `setAttribute` shape) —
    /// `None` degrades a nudge to a no-op rather than guessing. A no-op (nothing selected, design
    /// mode off, no relevant key held, or a nudge with no Anchor child selected — flow children
    /// have no `X`/`Y` to nudge, `DESIGNER.md` §4) returns an empty `Vec`.
    pub fn handle_keys(&mut self, layout: &LayoutMap, doc: Option<&ast::Document>, keys: DesignKeyInput) -> Vec<EditOp> {
        if !self.enabled {
            return Vec::new();
        }

        // Esc during an in-progress drag or marquee cancels the GESTURE (drops the local preview,
        // never touches text) rather than the ordinary Esc-to-parent selection change below.
        if keys.escape && (self.drag.is_some() || self.marquee.is_some()) {
            self.cancel_drag();
            return Vec::new();
        }

        if keys.select_all {
            self.select_all_siblings(layout);
            return Vec::new();
        }

        let Some(id) = self.selection.primary().map(str::to_string) else {
            return Vec::new();
        };

        if keys.escape {
            // The root (the view itself) has no parent: it stays selected.
            if let Some(parent) = layout.get(&id).and_then(|e| e.parent_id.clone()) {
                self.selection.set_single(Some(parent));
            } else if !id.is_empty() {
                self.selection.clear();
            }
            return Vec::new();
        }

        if keys.delete {
            return top_level_ids(self.selection.ids())
                .into_iter()
                .map(|element_id| EditOp::RemoveElement { element_id })
                .collect();
        }

        let step = if keys.shift { 10.0 } else { 1.0 };
        let dx = (keys.right as i32 - keys.left as i32) as f32 * step;
        let dy = (keys.down as i32 - keys.up as i32) as f32 * step;
        if dx == 0.0 && dy == 0.0 {
            return Vec::new();
        }
        let Some(doc) = doc else {
            return Vec::new();
        };

        let mut ops = Vec::new();
        for member in self.anchor_members(layout) {
            let Some(element) = doc.resolve_id(&member) else { continue };
            if dx != 0.0 {
                ops.push(nudge_attr(&element, &member, "X", dx));
            }
            if dy != 0.0 {
                ops.push(nudge_attr(&element, &member, "Y", dy));
            }
        }
        ops
    }

    /// Ctrl+A: every child of the primary selection's parent (every child of the root when the
    /// root, or nothing, is selected); the primary stays when it is among them.
    pub fn select_all_siblings(&mut self, layout: &LayoutMap) {
        let parent = match self.selection.primary() {
            Some(id) if !id.is_empty() => parent_id_of(id).unwrap_or_default(),
            _ => String::new(),
        };
        let ids: Vec<String> =
            layout.entries().iter().filter(|e| e.parent_id.as_deref() == Some(parent.as_str())).map(|e| e.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let primary = self.selection.primary().map(str::to_string);
        self.selection.set_many(ids, primary);
    }

    /// The top-level selected elements recorded as children of a Dock/Anchor container - what a
    /// move drag, a nudge or a group resize moves (a flow child has no `X`/`Y`; a `Locked` one stays put).
    fn anchor_members(&self, layout: &LayoutMap) -> Vec<String> {
        top_level_ids(self.selection.ids())
            .into_iter()
            .filter(|id| layout.get(id).is_some_and(|e| e.layout == LayoutKind::DockAnchor && !e.locked))
            .collect()
    }

    // ── DSG-9: move/resize/reorder drag; §13: marquee ──────────────────

    /// Whether a drag (move, resize, reorder) or a marquee is currently in progress —
    /// what a caller (`examples/view_embed.rs`) checks to decide whether to
    /// paint the previews and whether the next mouse-up should call [`Self::end_drag`].
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some() || self.marquee.is_some()
    }

    /// [`Self::press_with`] without modifiers.
    pub fn press(&mut self, layout: &LayoutMap, x: f32, y: f32) -> bool {
        self.press_with(layout, x, y, PointerModifiers::default())
    }

    /// A completed press at `(x, y)` (`DESIGNER.md` §13, WinForms semantics):
    ///
    /// - on a resize handle of the PRIMARY selection (an Anchor child, Ctrl not held): arms a
    ///   `Resize` immediately, `confirmed`, applying to every selected Anchor child;
    /// - on the empty area of a container that is not already selected and draggable (the view's
    ///   root always): selects the container (no modifier) and arms a marquee inside it — Ctrl
    ///   makes the marquee toggle, Shift add;
    /// - on anything else: Ctrl toggles it, Shift adds it, a plain press selects it (keeping the
    ///   multi-selection, with it as the primary, when it was already selected) and — when it is
    ///   still selected and draggable — ARMS the gesture in the same press: a group `Move` for an
    ///   Anchor child, a `Reorder` for a lone Flow child. The armed session starts
    ///   `confirmed: false` and produces nothing until [`Self::update_drag`] sees the pointer cross
    ///   [`DRAG_THRESHOLD`], so a plain click still just selects.
    ///
    /// Returns whether the selection itself changed.
    pub fn press_with(&mut self, layout: &LayoutMap, x: f32, y: f32, mods: PointerModifiers) -> bool {
        if !self.enabled {
            return false;
        }
        let before = self.selection.clone();
        self.drag = None;
        self.marquee = None;
        let modifier = mods.ctrl || mods.shift;

        // Shift only suppresses snapping during the drag: a Shift+press on a handle still resizes.
        if !mods.ctrl {
            if let Some(id) = self.selection.primary().map(str::to_string) {
                if let Some(entry) = layout.get(&id) {
                    if entry.layout == LayoutKind::DockAnchor && !entry.locked {
                        if let Some(handle) = handle_at(selection_frame(entry.bounds), RESIZE_HANDLE_SIZE, x, y) {
                            self.begin_group(layout, DragKind::Resize(handle), &id, x, y, true);
                            return false; // Selection unchanged.
                        }
                    }
                }
            }
        }

        let Some(hit) = layout.hit_test(x, y) else {
            if !modifier {
                self.selection.clear();
            }
            return before != self.selection;
        };
        let hit_id = hit.id.clone();

        let movable = !hit_id.is_empty() && matches!(hit.layout, LayoutKind::DockAnchor | LayoutKind::Flow);
        if hit.container && !(movable && before.contains(&hit_id) && !modifier) {
            self.begin_marquee(hit_id, x, y, mods);
            return before != self.selection;
        }

        if mods.ctrl {
            self.selection.toggle(hit_id.clone());
        } else if mods.shift {
            self.selection.add(hit_id.clone());
        } else if !self.selection.make_primary(&hit_id) {
            self.selection.set_single(Some(hit_id.clone()));
        }

        if self.selection.contains(&hit_id) {
            match hit.layout {
                LayoutKind::DockAnchor => self.begin_group(layout, DragKind::Move, &hit_id, x, y, false),
                LayoutKind::Flow if self.selection.len() == 1 && !hit.locked => self.begin_reorder(layout, hit_id, hit, x, y, false),
                _ => {}
            }
        }

        before != self.selection
    }

    /// Starts a marquee inside `container_id` from `(x, y)` - also what a press on the design
    /// canvas around the view does (the view's root). Without a modifier the container itself is
    /// selected right away (a click on its empty area selects it, like WinForms).
    pub fn begin_marquee(&mut self, container_id: String, x: f32, y: f32, mods: PointerModifiers) {
        if !self.enabled {
            return;
        }
        self.drag = None;
        let mode = if mods.ctrl {
            MarqueeMode::Toggle
        } else if mods.shift {
            MarqueeMode::Add
        } else {
            MarqueeMode::Replace
        };
        if mode == MarqueeMode::Replace {
            self.selection.set_single(Some(container_id.clone()));
        }
        self.marquee = Some(MarqueeSession { container_id, start: (x, y), current: (x, y), mode, confirmed: false, hits: Vec::new() });
    }

    /// Arms a `Move`/`Resize` over every selected Anchor child, `primary` being the pressed one.
    fn begin_group(&mut self, layout: &LayoutMap, kind: DragKind, primary: &str, x: f32, y: f32, confirmed: bool) {
        let Some(entry) = layout.get(primary) else { return };
        let members: Vec<(String, Rect, Rect)> = self
            .anchor_members(layout)
            .into_iter()
            .filter_map(|id| layout.get(&id).map(|e| (id, e.bounds, e.bounds)))
            .collect();
        if !members.iter().any(|(id, _, _)| id == primary) {
            return;
        }
        self.drag = Some(DragSession {
            kind,
            element_id: primary.to_string(),
            parent_id: entry.parent_id.clone(),
            start_mouse: (x, y),
            start_bounds: entry.bounds,
            live_bounds: entry.bounds,
            members,
            guides: Vec::new(),
            reorder_index: 0,
            reorder_original_index: 0,
            menu_target: None,
            confirmed,
        });
    }

    fn begin_reorder(&mut self, layout: &LayoutMap, id: String, entry: &LayoutEntry, x: f32, y: f32, confirmed: bool) {
        // The dragged element's own starting index among ALL of its
        // parent's children (itself included, unlike `sibling_bounds` below)
        // — what `end_drag` compares the final `reorder_index` against to
        // decide whether anything actually moved.
        let original_index =
            layout.entries().iter().filter(|e| e.parent_id == entry.parent_id).position(|e| e.id == id).unwrap_or(0);
        self.drag = Some(DragSession {
            kind: DragKind::Reorder,
            element_id: id,
            parent_id: entry.parent_id.clone(),
            start_mouse: (x, y),
            start_bounds: entry.bounds,
            live_bounds: entry.bounds,
            members: Vec::new(),
            guides: Vec::new(),
            reorder_index: original_index,
            reorder_original_index: original_index,
            menu_target: None,
            confirmed,
        });
    }

    /// Updates the in-progress drag or marquee from the pointer's current position —
    /// harmless (a no-op) when [`Self::is_dragging`] is `false`. An ARMED but
    /// not yet `confirmed` session does nothing at all until the pointer crosses
    /// [`DRAG_THRESHOLD`] from the press point, at which point it becomes `confirmed` and this
    /// same call already applies the full delta from the ORIGINAL press — no jump.
    /// `suppress_snap` (Shift held) skips [`snap_bounds`] entirely for a `Move`/`Resize` session
    /// (`DESIGNER.md` §4's "Shift suppresses snapping, as WinForms does"). A group move snaps the
    /// bounds of the whole group against the primary's siblings that are not being moved.
    pub fn update_drag(&mut self, layout: &LayoutMap, x: f32, y: f32, suppress_snap: bool) {
        if let Some(marquee) = self.marquee.as_mut() {
            marquee.current = (x, y);
            if !marquee.confirmed
                && (x - marquee.start.0).abs() < DRAG_THRESHOLD
                && (y - marquee.start.1).abs() < DRAG_THRESHOLD
            {
                return;
            }
            marquee.confirmed = true;
            marquee.hits = marquee_hits(layout, &marquee.container_id, marquee_rect(marquee.start, marquee.current));
            return;
        }

        let Some(session) = self.drag.as_mut() else { return };
        let dx = x - session.start_mouse.0;
        let dy = y - session.start_mouse.1;

        if !session.confirmed {
            if dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
                return;
            }
            session.confirmed = true;
        }

        let excluded: Vec<String> = session.members.iter().map(|(id, _, _)| id.clone()).collect();
        match session.kind {
            DragKind::Move => {
                let Some(group) = union_rect(session.members.iter().map(|(_, start, _)| *start)) else { return };
                let raw = move_rect(group, dx, dy);
                let (snapped, guides) = if suppress_snap {
                    (raw, Vec::new())
                } else {
                    snap_bounds(raw, &snap_candidates(layout, &session.parent_id, &excluded), SNAP_THRESHOLD)
                };
                let (ddx, ddy) = (snapped.left - group.left, snapped.top - group.top);
                for (_, start, live) in session.members.iter_mut() {
                    *live = move_rect(*start, ddx, ddy);
                }
                session.live_bounds = move_rect(session.start_bounds, ddx, ddy);
                session.guides = guides;
            }
            DragKind::Resize(handle) => {
                let raw = resize_rect(session.start_bounds, handle, dx, dy);
                let (snapped, guides) = if suppress_snap {
                    (raw, Vec::new())
                } else {
                    snap_bounds(raw, &snap_candidates(layout, &session.parent_id, &excluded), SNAP_THRESHOLD)
                };
                let s = session.start_bounds;
                let deltas = (snapped.left - s.left, snapped.top - s.top, snapped.right - s.right, snapped.bottom - s.bottom);
                for (_, start, live) in session.members.iter_mut() {
                    *live = apply_edge_deltas(*start, deltas);
                }
                session.live_bounds = snapped;
                session.guides = guides;
            }
            DragKind::Reorder => {
                // A menu row may also move to another level of its menu (MENUS.md): into a sub-menu, out of one.
                session.menu_target = None;
                if crate::menus::is_menu_row(&session.element_id) {
                    if let Some(target) = crate::menus::drop_target(&session.element_id, x, y) {
                        session.menu_target = Some(target);
                        return;
                    }
                }
                let siblings = sibling_bounds(layout, &session.parent_id, std::slice::from_ref(&session.element_id));
                session.reorder_index = flow_insertion_index(&siblings, x, y);
            }
        }
    }

    /// The current live ghost rect of the PRIMARY element for a `Move`/`Resize` drag — `None` while
    /// not dragging, during a `Reorder` (which has [`Self::reorder_marker`] instead), or while an
    /// armed session has not yet crossed [`DRAG_THRESHOLD`] (so a plain click never flashes a ghost).
    pub fn drag_preview(&self) -> Option<Rect> {
        self.drag
            .as_ref()
            .filter(|s| s.confirmed && matches!(s.kind, DragKind::Move | DragKind::Resize(_)))
            .map(|s| s.live_bounds)
    }

    /// Every moved/resized element's live ghost rect (the primary's included), empty when
    /// [`Self::drag_preview`] is `None`.
    pub fn drag_previews(&self) -> Vec<Rect> {
        self.drag
            .as_ref()
            .filter(|s| s.confirmed && matches!(s.kind, DragKind::Move | DragKind::Resize(_)))
            .map(|s| s.members.iter().map(|(_, _, live)| *live).collect())
            .unwrap_or_default()
    }

    /// The snaplines active in the current `Move`/`Resize` drag — always
    /// empty outside one.
    pub fn drag_guides(&self) -> &[SnapGuide] {
        self.drag.as_ref().map(|s| s.guides.as_slice()).unwrap_or(&[])
    }

    /// The marquee rectangle of an in-progress marquee selection, once past [`DRAG_THRESHOLD`].
    pub fn marquee_rect(&self) -> Option<Rect> {
        self.marquee.as_ref().filter(|m| m.confirmed).map(|m| marquee_rect(m.start, m.current))
    }

    /// The elements the in-progress marquee touches right now (what its release will select).
    pub fn marquee_hits(&self) -> &[String] {
        self.marquee.as_ref().map(|m| m.hits.as_slice()).unwrap_or(&[])
    }

    /// The Flow insertion marker for the current `Reorder` drag — `None`
    /// while not reordering, or when `layout` has no entry for the parent
    /// container any more (a reload raced the drag).
    pub fn reorder_marker(&self, layout: &LayoutMap) -> Option<Rect> {
        let session = self.drag.as_ref()?;
        if session.kind != DragKind::Reorder || !session.confirmed {
            return None;
        }
        if let Some((_, _, marker)) = &session.menu_target {
            return Some(*marker);
        }
        let container = layout.get(session.parent_id.as_deref()?)?.bounds;
        let siblings = sibling_bounds(layout, &session.parent_id, std::slice::from_ref(&session.element_id));
        Some(flow_insertion_marker(&siblings, container, session.reorder_index, 3.0))
    }

    /// Cancels the in-progress drag or marquee with no [`EditOp`] — `DESIGNER.md` DSG-9
    /// item 1's "Esc during a drag cancels". Since a drag never writes text
    /// before [`Self::end_drag`], cancelling is simply dropping the session; nothing needs
    /// reverting (a marquee never changed the selection before its release).
    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.marquee = None;
    }

    /// Ends the in-progress drag or marquee (mouse-up), consuming it — `None` outcome
    /// while not dragging. A marquee changes the selection and never produces an edit. `doc` is
    /// the current document's parsed AST, needed only for a `Move`/`Resize` session (to read each
    /// element's CURRENT literal `X`/`Y`/`Width`/`Height` before offsetting — the same
    /// "absolute value, never a delta" rule [`Self::handle_keys`]'s own nudge uses); `None`
    /// degrades a `Move`/`Resize` end to a no-op. A `Reorder` session needs no `doc` at all.
    pub fn end_drag(&mut self, doc: Option<&ast::Document>) -> DragOutcome {
        if let Some(marquee) = self.marquee.take() {
            self.end_marquee(marquee);
            return DragOutcome::None;
        }

        let Some(session) = self.drag.take() else { return DragOutcome::None };

        match session.kind {
            DragKind::Move | DragKind::Resize(_) => {
                if !session.confirmed {
                    return DragOutcome::None;
                }
                let Some(doc) = doc else { return DragOutcome::None };
                const EPS: f32 = 0.001;
                let mut ops = Vec::new();
                for (id, start, live) in &session.members {
                    let Some(element) = doc.resolve_id(id) else { continue };
                    let dx = live.left - start.left;
                    let dy = live.top - start.top;
                    let dw = (live.right - live.left) - (start.right - start.left);
                    let dh = (live.bottom - live.top) - (start.bottom - start.top);
                    for (name, delta) in [("X", dx), ("Y", dy), ("Width", dw), ("Height", dh)] {
                        if delta.abs() > EPS {
                            ops.extend(drag_attr(&element, id, name, delta));
                        }
                    }
                }
                if ops.is_empty() {
                    return DragOutcome::None;
                }
                let gesture = if matches!(session.kind, DragKind::Move) { Gesture::Move } else { Gesture::Resize };
                DragOutcome::Batch { ops, gesture }
            }
            DragKind::Reorder => {
                if let (Some((parent, index, _)), true) = (session.menu_target.clone(), session.confirmed) {
                    // A menu row: its own ordinal is its index in its level (its id's last step).
                    let ordinal = session.element_id.rsplit('.').next().and_then(|s| s.parse::<usize>().ok());
                    if session.parent_id.as_deref() == Some(parent.as_str()) && ordinal == Some(index) {
                        return DragOutcome::None;
                    }
                    return DragOutcome::Single(EditOp::MoveElement { element_id: session.element_id, new_parent_id: parent, index });
                }
                if session.reorder_index == session.reorder_original_index {
                    return DragOutcome::None;
                }
                let Some(parent_id) = session.parent_id else { return DragOutcome::None };
                DragOutcome::Single(EditOp::MoveElement {
                    element_id: session.element_id,
                    new_parent_id: parent_id,
                    index: session.reorder_index,
                })
            }
        }
    }

    /// A marquee's release: the touched children replace the selection (plain drag, the container
    /// stays selected when it touched nothing), are toggled (Ctrl) or added (Shift); a Ctrl/Shift
    /// click that never became a marquee toggles/adds the container itself.
    fn end_marquee(&mut self, marquee: MarqueeSession) {
        match (marquee.mode, marquee.confirmed) {
            (MarqueeMode::Replace, true) if !marquee.hits.is_empty() => {
                let primary = marquee.hits.first().cloned();
                self.selection.set_many(marquee.hits, primary);
            }
            (MarqueeMode::Replace, _) => {}
            (MarqueeMode::Toggle, true) => marquee.hits.into_iter().for_each(|id| self.selection.toggle(id)),
            (MarqueeMode::Add, true) => marquee.hits.into_iter().for_each(|id| self.selection.add(id)),
            (MarqueeMode::Toggle, false) => self.selection.toggle(marquee.container_id),
            (MarqueeMode::Add, false) => self.selection.add(marquee.container_id),
        }
    }
}

/// What ending a drag ([`DesignController::end_drag`]) produced — the shape
/// a caller (`examples/view_embed.rs`) matches on to decide which
/// `SurfaceMessage` to send: a single op as the existing `editRequest`
/// (matching Delete/nudge), a batch as the new DSG-9 `editRequests`.
#[derive(Debug, Clone, PartialEq)]
pub enum DragOutcome {
    None,
    Single(EditOp),
    Batch { ops: Vec<EditOp>, gesture: Gesture },
}

/// The direct children's own painted bounds of `parent_id`, the `excluded` ones (the dragged
/// elements) left out — the sibling geometry [`flow_insertion_index`]/
/// [`flow_insertion_marker`] and [`snap_bounds`]'s own candidates are built
/// from.
fn sibling_bounds(layout: &LayoutMap, parent_id: &Option<String>, excluded: &[String]) -> Vec<Rect> {
    let Some(pid) = parent_id else { return Vec::new() };
    layout
        .entries()
        .iter()
        .filter(|e| e.parent_id.as_deref() == Some(pid.as_str()) && !excluded.contains(&e.id))
        .map(|e| e.bounds)
        .collect()
}

/// [`sibling_bounds`] plus the container's OWN outer bounds — the snap
/// candidate set for a `Move`/`Resize` drag (`DESIGNER.md` DSG-9 item 1's
/// "sibling edges/centres AND container padding" — see [`snap_bounds`]'s own
/// doc for why a container candidate is its outer bounds, the padding-`0`
/// line).
fn snap_candidates(layout: &LayoutMap, parent_id: &Option<String>, excluded: &[String]) -> Vec<Rect> {
    let mut out = sibling_bounds(layout, parent_id, excluded);
    if let Some(pid) = parent_id {
        if let Some(container) = layout.get(pid) {
            out.push(container.bounds);
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────
// DSG-9: toolbox drop — host→surface `dragEnter`/`dragOver`/`drop`/
// `dragLeave` (`crate::protocol::HostMessage`), independent of click
// selection above (a toolbox drag has no `DesignController` selection of its
// own — the dragged thing is a NEW, not-yet-inserted component).
// ─────────────────────────────────────────────────────────────────────────

/// A new element's default size when dropped into an Anchor/absolute container, DIP - what the
/// Windows Forms designer does with a control's `DefaultSize` (a Button lands 75×23, a TextBox
/// 100×23): a size the control looks right at, here matched to the Kubuno controls' own metrics
/// (36 DIP buttons and fields). `vskubuno`'s `ToolboxInsertionPlanner.DefaultSize` mirrors this
/// table for a Toolbox double-click.
pub fn default_drop_size(component: &str) -> (f32, f32) {
    match component {
        "Button" | "IconButton" => (100.0, 36.0),
        "TextField" | "SearchField" | "MaskedField" | "NumericField" | "ColorField" | "GradientField" | "DatePicker" | "ComboBox"
        | "Dropdown" => (200.0, 36.0),
        "Label" | "LinkLabel" | "Badge" => (100.0, 24.0),
        "CheckBox" | "RadioButton" | "Switch" => (140.0, 24.0),
        "Slider" | "ProgressBar" => (200.0, 24.0),
        "Separator" => (200.0, 8.0),
        "Icon" | "Spinner" => (24.0, 24.0),
        "TextArea" | "ListBox" | "CheckedListBox" | "ListView" | "TreeView" | "DataTable" => (200.0, 120.0),
        "MonthCalendar" => (280.0, 300.0),
        // A user control: the size it was designed at (its view's `DesignWidth` × `DesignHeight`),
        // as Windows Forms drops a UserControl at its own default `Size`.
        other => match (user_control_design_size(other), registry::lookup(other)) {
            (Some(size), _) => size,
            // A container: room to drop something into it.
            (None, Some(meta)) if meta.children != ChildrenModel::None => (200.0, 100.0),
            _ => (120.0, 36.0),
        },
    }
}

/// The `DesignWidth` × `DesignHeight` of a linked user control's own view (`None` for anything
/// else, or a view without them).
pub fn user_control_design_size(component: &str) -> Option<(f32, f32)> {
    use crate::ast::AstNode;
    let view = registry::project::project_info(component)?.view?;
    let parse = crate::syntax::parse(view);
    let root = crate::ast::Document::cast(parse.syntax())?.root_element()?;
    let number = |name: &str| root.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0);
    Some((number("DesignWidth")?, number("DesignHeight")?))
}

/// The attribute every element dropped into an Anchor container gets, WinForms' own default
/// anchoring (`AnchorStyles.Top | AnchorStyles.Left`), written out so the Properties window shows it.
pub const DEFAULT_DROP_ANCHOR: &str = "Top, Left";

/// Where a toolbox drop would land right now — computed by
/// [`ToolboxController::drag_over`], consumed by
/// [`ToolboxController::drop`] and painted by [`paint_drop_marker`].
#[derive(Clone)]
pub struct DropTarget {
    pub parent_id: String,
    /// The insertion index for a Flow parent, or the (already validity-
    /// checked) append index (`children.len()`) for anything else —
    /// [`crate::edit::insert_child`]'s own "at or past the current child
    /// count appends" contract makes an always-valid index cheap to compute
    /// even when a Flow-specific index would not otherwise apply.
    pub index: usize,
    /// `Some((x, y))` — the new element's `X`/`Y` attribute values, parent-
    /// LOCAL DIP — only when the parent's own registry [`LayoutKind`] is
    /// `DockAnchor`; `None` for a Flow/other parent (no `X`/`Y` to set).
    pub xy: Option<(f32, f32)>,
    /// Whether `component` (the string [`ToolboxController::drag_enter`] was
    /// given) is actually allowed here — [`can_drop_component`]'s own
    /// registry check. `DESIGNER.md` DSG-9 item 3's "not allowed" marker.
    pub valid: bool,
    /// What [`paint_drop_marker`] paints: an outline ghost of the new
    /// element's placement for `xy.is_some()`, an insertion line otherwise.
    pub marker: Rect,
}

/// Host-driven toolbox drag/drop (`DESIGNER.md` DSG-9 item 3): the VS
/// Toolbox drag is OLE/WPF on the host side, translated into these four
/// plain messages before reaching the surface at all — this controller only
/// ever sees `(component, x, y)`, never a live OS drag object.
#[derive(Default)]
pub struct ToolboxController {
    component: Option<String>,
    target: Option<DropTarget>,
    /// The window's title band as the designer draws it ([`ToolboxController::set_title_band`]).
    band: Option<kubuno_desktop_controls::window_chrome::ChromeLayout>,
    /// The title-bar region the current target drops into (`TitleBar.Region`).
    band_region: Option<&'static str>,
}

impl ToolboxController {
    pub fn new() -> Self {
        Self::default()
    }

    /// `kubuno/dragEnter {component}` — `component` is a registry element
    /// name (`"Button"`), not yet validated against anything (that happens
    /// per-position in [`Self::drag_over`], since validity depends on WHERE
    /// the pointer is).
    pub fn drag_enter(&mut self, component: String) {
        self.component = Some(component);
        self.target = None;
    }

    /// `kubuno/dragLeave` — clears the drag entirely (a fresh `dragEnter`
    /// must follow before another `dragOver`/`drop` does anything).
    pub fn drag_leave(&mut self) {
        self.component = None;
        self.target = None;
        self.band_region = None;
    }

    /// The window's title band as the designer drew it this frame (`None`: the view has none): a
    /// drop over it goes to one of its regions.
    pub fn set_title_band(&mut self, band: Option<kubuno_desktop_controls::window_chrome::ChromeLayout>) {
        self.band = band;
    }

    /// While a control is dragged from the Toolbox over a view with a title band: the band's three drop zones
    /// (`TitleBar.Region` Left, Center, Right — [`band_drop_zones`]) and the one the pointer is over, for
    /// [`paint_band_drop_zones`].
    pub fn band_zones(&self) -> Option<(BandZones, Option<&'static str>)> {
        let component = self.component.as_deref()?;
        let band = self.band.as_ref().filter(|_| !registry::is_non_visual(component))?;
        Some((band_drop_zones(band), self.band_region))
    }

    pub fn is_active(&self) -> bool {
        self.component.is_some()
    }

    pub fn target(&self) -> Option<&DropTarget> {
        self.target.as_ref()
    }

    /// `kubuno/dragOver {x, y}`: recomputes and stores the current drop
    /// target, returning it (`None` for no active drag, no element under the
    /// point, or a hit whose leaf has no recorded parent at all — never
    /// expected once there is a root element).
    ///
    /// Routing rule: if the element directly under the pointer itself
    /// accepts children (its own registry `ChildrenModel != None`), the drop
    /// targets THAT element (dropping "into" it); otherwise it targets the
    /// hit element's own PARENT (dropping "near" a leaf targets its
    /// container) — the same rule a WinForms-class designer uses to decide
    /// "drop into this container" vs. "drop next to this control".
    pub fn drag_over(&mut self, layout: &LayoutMap, doc: &ast::Document, x: f32, y: f32) -> Option<&DropTarget> {
        let component = self.component.clone()?;
        self.band_region = None;
        // Over the window's title band: into the region under the pointer (`TitleBar.Region`), as a
        // child of the view's root.
        if let Some(band) = self.band.as_ref().filter(|b| b.band.contains(x, y) && !registry::is_non_visual(&component)) {
            let root = doc.root_element()?;
            let root_name = root.name().unwrap_or_default();
            let count = root.children().count();
            let zones = band_drop_zones(band);
            // The zone under the pointer, else the nearest one (over the icon or the caption buttons).
            let distance = |z: &Rect| if x < z.left { z.left - x } else if x >= z.right { x - z.right } else { 0.0 };
            let (region, zone) = zones.iter().copied().min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1))).unwrap_or(zones[1]);
            let marker = match region {
                "Left" => band.left,
                "Center" => band.center,
                _ => band.right,
            };
            // An empty region: the ghost of the new control where the pointer is, inside the zone.
            let marker = if marker.right - marker.left < 8.0 {
                let half = 18.0f32.min((zone.right - zone.left) / 2.0);
                let cx = x.clamp(zone.left + half, zone.right - half);
                Rect::new(cx - half, zone.top + 2.0, cx + half, zone.bottom - 2.0)
            } else {
                marker
            };
            let valid = can_drop_component(&root_name, count, &component)
                && registry::lookup(&root_name).is_some_and(|m| m.layout == LayoutKind::DockAnchor);
            self.band_region = Some(region);
            self.target = Some(DropTarget { parent_id: String::new(), index: count, xy: None, valid, marker });
            return self.target.as_ref();
        }
        let hit = layout.hit_test(x, y)?;
        let hit_id = hit.id.clone();
        let hit_element = doc.resolve_id(&hit_id)?;
        let hit_name = hit_element.name().unwrap_or_default();
        let hit_accepts_children =
            registry::lookup(&hit_name).map(|m| m.children != ChildrenModel::None).unwrap_or(false);

        let (container_id, container_bounds) = if hit_accepts_children {
            (hit_id, hit.bounds)
        } else {
            let pid = hit.parent_id.clone()?;
            let bounds = layout.get(&pid)?.bounds;
            (pid, bounds)
        };
        let container_element = doc.resolve_id(&container_id)?;
        let container_name = container_element.name().unwrap_or_default();
        let container_meta = registry::lookup(&container_name);
        let children_count = container_element.children().count();
        let siblings: Vec<Rect> = layout
            .entries()
            .iter()
            .filter(|e| e.parent_id.as_deref() == Some(container_id.as_str()))
            .map(|e| e.bounds)
            .collect();

        let (index, xy, marker) = match container_meta.map(|m| m.layout).unwrap_or(LayoutKind::None) {
            LayoutKind::DockAnchor => {
                // Whole DIP, like every other gesture of the designer (WinForms' pixels).
                let local_x = (x - container_bounds.left).max(0.0).round();
                let local_y = (y - container_bounds.top).max(0.0).round();
                let (width, height) = default_drop_size(&component);
                let (left, top) = (container_bounds.left + local_x, container_bounds.top + local_y);
                let marker = Rect::new(left, top, left + width, top + height);
                (children_count, Some((local_x, local_y)), marker)
            }
            LayoutKind::Flow => {
                let index = flow_insertion_index(&siblings, x, y);
                let marker = flow_insertion_marker(&siblings, container_bounds, index, 3.0);
                (index, None, marker)
            }
            _ => (children_count, None, container_bounds),
        };

        let valid = can_drop_component(&container_name, children_count, &component);
        self.target = Some(DropTarget { parent_id: container_id, index, xy, valid, marker });
        self.target.as_ref()
    }

    /// `kubuno/drop {x, y}` — consumes the drag entirely (a fresh
    /// `dragEnter` is needed for another drop), returning the
    /// [`EditOp::InsertChild`] the host forwards to `kubuno-desktop-views-ls`'s
    /// `kubuno/applyEdit`, or `None` when there is no active drag, no
    /// computed target (`drag_over` was never called for this position), or
    /// the current target is [`DropTarget::valid`] `false`.
    pub fn drop(&mut self) -> Option<EditOp> {
        let component = self.component.take()?;
        let target = self.target.take()?;
        if !target.valid {
            return None;
        }
        let xml = match self.band_region.take() {
            Some(region) => {
                // The web header's 36 DIP buttons (`w-9 h-9`), shorter in a band too low for them.
                let height = self.band.as_ref().map(|b| (b.band.bottom - b.band.top - 8.0).clamp(16.0, 36.0)).unwrap_or(30.0);
                let (width, _) = default_drop_size(&component);
                if component == "IconButton" {
                    // The header's round buttons: the caption buttons' size, 36 in the tall header.
                    let glyph = if height >= 36.0 { 18.0 } else { 16.0 };
                    let side = format_dip(height);
                    format!(r#"<IconButton TitleBar.Region="{region}" Diameter="{side}" Glyph="{}" Width="{side}" Height="{side}"/>"#, format_dip(glyph))
                } else {
                    format!(r#"<{component} TitleBar.Region="{region}" Width="{}" Height="{}"/>"#, format_dip(width.min(200.0)), format_dip(height))
                }
            }
            None => skeleton_xml(&component, target.xy),
        };
        Some(EditOp::InsertChild { parent_id: target.parent_id, index: target.index, xml })
    }
}

/// A title band's drop zones: each `TitleBar.Region` with its rectangle ([`band_drop_zones`]).
pub type BandZones = [(&'static str, Rect); 3];

/// The title band's three drop zones, in `TitleBar.Region` order (Left, Center, Right): the band between the
/// window's icon and its caption buttons cut in thirds — the centre zone around the band's middle, where the centre
/// region is — each zone given to the region on its side (mirrored for a right-to-left window, whose caption
/// buttons are on the left). Inset vertically, so the zones read as targets inside the band.
pub fn band_drop_zones(band: &kubuno_desktop_controls::window_chrome::ChromeLayout) -> [(&'static str, Rect); 3] {
    let b = band.band;
    let middle = (b.left + b.right) / 2.0;
    let mirrored = band.left.left > middle;
    // From the icon (or the band's inset) to the caption buttons.
    let (start, end) = if mirrored { (band.right.left, band.left.right) } else { (band.left.left, band.right.right) };
    let (start, end) = if end > start { (start, end) } else { (b.left, b.right) };
    let third = (end - start) / 3.0;
    let (top, bottom) = (b.top + 4.0, b.bottom - 4.0);
    let near = Rect::new(start, top, start + third, bottom);
    let centre = Rect::new(start + third, top, end - third, bottom);
    let far = Rect::new(end - third, top, end, bottom);
    if mirrored {
        [("Left", far), ("Center", centre), ("Right", near)]
    } else {
        [("Left", near), ("Center", centre), ("Right", far)]
    }
}

/// The title band's drop zones while a Toolbox control is dragged over the view: a dashed outline around each, the
/// one under the pointer washed with the accent — the band's regions are where the control can go.
pub fn paint_band_drop_zones(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &kubuno_desktop_ui::Theme, zones: &[(&'static str, Rect); 3], hot: Option<&str>) {
    for (region, zone) in zones {
        if zone.right - zone.left < 4.0 {
            continue;
        }
        if hot == Some(*region) {
            let mut wash = theme.accent;
            wash.a = 0.16;
            c.fill_rounded(zone, 4.0, &wash);
        }
        let ink = if hot == Some(*region) { theme.accent } else { theme.text_secondary };
        for dash in dashed_outline(*zone, 4.0, 3.0, 1.0) {
            c.fill_rect(&dash, &ink);
        }
    }
}

/// Whether `component` may be inserted as one of `container_name`'s
/// children, given it already has `existing_children` — `DESIGNER.md` DSG-9
/// item 3's "Validate against the registry's allowed children (typed
/// ChildrenModel)". Mirrors `crate::validate`'s own gating rule (a gated
/// child name — one that appears in SOME component's own `allowed` list
/// anywhere in the registry — is only valid directly under a parent whose
/// own `allowed` names it; an ungated name is fine under any `List`
/// container) at the single-candidate granularity a drop needs, rather than
/// reusing that module's own whole-document walk (`crate::validate::walk` is
/// private, and built to visit every element, not to answer one "would this
/// ONE candidate be accepted here" question).
fn can_drop_component(container_name: &str, existing_children: usize, component: &str) -> bool {
    let Some(meta) = registry::lookup(container_name) else { return false };
    if registry::lookup(component).is_none() {
        return false;
    }
    match meta.children {
        ChildrenModel::None => false,
        ChildrenModel::SingleWidget => existing_children == 0,
        ChildrenModel::List(allowed) => {
            if is_gated_anywhere(component) {
                allowed.contains(&component)
            } else {
                true
            }
        }
    }
}

/// Whether `name` is some OTHER component's gated child — see
/// `crate::validate`'s identically-purposed (and identically named, in
/// spirit) private helper; duplicated here in miniature rather than made
/// `pub(crate)` there, since this module owns no other coupling to
/// `validate.rs` and the whole check is three lines.
fn is_gated_anywhere(name: &str) -> bool {
    registry::all().iter().any(|c| matches!(c.children, ChildrenModel::List(allowed) if allowed.contains(&name)))
}

/// The new element's `.kbview` skeleton for [`ToolboxController::drop`] —
/// `<Component/>` for a Flow/other parent, `<Component X="…" Y="…"
/// Width="…" Height="…" Anchor="Top, Left"/>` for an Anchor one: placed at the
/// drop point with its [`default_drop_size`] and WinForms' default anchoring.
pub fn skeleton_xml(component: &str, xy: Option<(f32, f32)>) -> String {
    // A ribbon element arrives with what makes it usable (a ribbon docked on top with a tab, a
    // group and a button; a group with a button…).
    if let Some(xml) = crate::registry::families::ribbon::skeleton(component) {
        return xml;
    }
    // A non-visual component (a `<Timer>`, EVT-7b) has no place on the surface: the component tray.
    let xy = if registry::is_non_visual(component) { None } else { xy };
    match xy {
        Some((x, y)) => {
            let (width, height) = default_drop_size(component);
            format!(
                r#"<{component} X="{}" Y="{}" Width="{}" Height="{}" Anchor="{DEFAULT_DROP_ANCHOR}"/>"#,
                format_dip(x),
                format_dip(y),
                format_dip(width),
                format_dip(height)
            )
        }
        None => format!("<{component}/>"),
    }
}

/// The `setAttribute` a drag's end writes for one axis: the attribute's current literal value plus
/// `delta`, rounded to a whole DIP like the Windows Forms designer's pixels (a pointer delta at a
/// fractional scale is fractional); `None` when the rounded value does not change.
fn drag_attr(element: &ast::Element, id: &str, name: &str, delta: f32) -> Option<EditOp> {
    let current = element.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
    let value = (current + delta).round();
    (value != current).then(|| EditOp::SetAttribute { element_id: id.to_string(), name: name.to_string(), value: format_dip(value) })
}

fn nudge_attr(element: &ast::Element, id: &str, name: &str, delta: f32) -> EditOp {
    let current = element
        .attribute(name)
        .and_then(|a| a.value())
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    EditOp::SetAttribute { element_id: id.to_string(), name: name.to_string(), value: format_dip(current + delta) }
}

// ─────────────────────────────────────────────────────────────────────────
// Layout ("Format") commands on a multi-selection (`vskubuno/docs/DESIGNER.md`
// §13): WinForms' Layout toolbar / Format menu - align, make same size,
// spacing, center in the container. Pure geometry over painted bounds.
// ─────────────────────────────────────────────────────────────────────────

/// One command of the Layout toolbar / Format menu, sent by the host as `format {command}`.
/// Every command applies to the top-level selected Anchor children that are not docked
/// ([`format_members`]); alignment and sizing are relative to the PRIMARY selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatCommand {
    AlignLefts,
    AlignCenters,
    AlignRights,
    AlignTops,
    AlignMiddles,
    AlignBottoms,
    MakeSameWidth,
    MakeSameHeight,
    MakeSameSize,
    HorizontalSpacingEqual,
    HorizontalSpacingIncrease,
    HorizontalSpacingDecrease,
    HorizontalSpacingRemove,
    VerticalSpacingEqual,
    VerticalSpacingIncrease,
    VerticalSpacingDecrease,
    VerticalSpacingRemove,
    CenterHorizontally,
    CenterVertically,
}

/// How much "Increase/Decrease Horizontal/Vertical Spacing" changes each gap, DIP (WinForms' default
/// grid size).
pub const SPACING_STEP: f32 = 8.0;

/// One element a format command moves or resizes: its id, painted bounds and its container's
/// painted bounds (its `X`/`Y` are measured from that box's origin, `DESIGNER.md` §12).
#[derive(Clone)]
pub struct FormatMember {
    pub id: String,
    pub bounds: Rect,
    pub parent_bounds: Rect,
}

/// The elements a format command applies to: the top-level selected elements ([`top_level_ids`])
/// placed by a Dock/Anchor container and not docked (a docked element's place comes from its
/// `Dock`), in selection order.
pub fn format_members(layout: &LayoutMap, doc: &ast::Document, selection: &Selection) -> Vec<FormatMember> {
    top_level_ids(selection.ids())
        .into_iter()
        .filter_map(|id| {
            let entry = layout.get(&id).filter(|e| e.layout == LayoutKind::DockAnchor)?;
            let parent = layout.get(entry.parent_id.as_deref()?)?;
            let element = doc.resolve_id(&id)?;
            let docked = element.attribute("Dock").and_then(|a| a.value()).is_some_and(|d| !d.trim().is_empty() && d.trim() != "None");
            (!docked).then_some(FormatMember { id, bounds: entry.bounds, parent_bounds: parent.bounds })
        })
        .collect()
}

/// The minimum number of [`format_members`] a command needs to do anything: 2 for align/size and
/// increase/decrease/remove spacing, 3 for "make spacing equal", 1 for centering.
pub fn format_min_members(command: FormatCommand) -> usize {
    use FormatCommand::*;
    match command {
        HorizontalSpacingEqual | VerticalSpacingEqual => 3,
        CenterHorizontally | CenterVertically => 1,
        _ => 2,
    }
}

/// The new bounds of every member (same order as `members`) after `command`; `primary` is the index
/// of the reference element (the primary selection). Unchanged bounds when the command does not
/// apply (too few members).
pub fn format_rects(members: &[FormatMember], primary: usize, command: FormatCommand) -> Vec<Rect> {
    use FormatCommand::*;
    let rects: Vec<Rect> = members.iter().map(|m| m.bounds).collect();
    if members.len() < format_min_members(command) || primary >= members.len() {
        return rects;
    }
    let p = rects[primary];
    let (pw, ph) = (p.right - p.left, p.bottom - p.top);
    let with_left = |r: Rect, left: f32| Rect::new(left, r.top, left + (r.right - r.left), r.bottom);
    let with_top = |r: Rect, top: f32| Rect::new(r.left, top, r.right, top + (r.bottom - r.top));
    match command {
        AlignLefts => rects.iter().map(|r| with_left(*r, p.left)).collect(),
        AlignCenters => rects.iter().map(|r| with_left(*r, (p.left + p.right) / 2.0 - (r.right - r.left) / 2.0)).collect(),
        AlignRights => rects.iter().map(|r| with_left(*r, p.right - (r.right - r.left))).collect(),
        AlignTops => rects.iter().map(|r| with_top(*r, p.top)).collect(),
        AlignMiddles => rects.iter().map(|r| with_top(*r, (p.top + p.bottom) / 2.0 - (r.bottom - r.top) / 2.0)).collect(),
        AlignBottoms => rects.iter().map(|r| with_top(*r, p.bottom - (r.bottom - r.top))).collect(),
        MakeSameWidth => rects.iter().map(|r| Rect::new(r.left, r.top, r.left + pw, r.bottom)).collect(),
        MakeSameHeight => rects.iter().map(|r| Rect::new(r.left, r.top, r.right, r.top + ph)).collect(),
        MakeSameSize => rects.iter().map(|r| Rect::new(r.left, r.top, r.left + pw, r.top + ph)).collect(),
        HorizontalSpacingEqual | HorizontalSpacingIncrease | HorizontalSpacingDecrease | HorizontalSpacingRemove => {
            space(&rects, primary, command, true)
        }
        VerticalSpacingEqual | VerticalSpacingIncrease | VerticalSpacingDecrease | VerticalSpacingRemove => {
            space(&rects, primary, command, false)
        }
        CenterHorizontally | CenterVertically => center(members, command == CenterHorizontally),
    }
}

/// Spacing along one axis: the members sorted along it, each gap replaced (equal: the average gap,
/// first and last staying put; increase/decrease: ±[`SPACING_STEP`], never below 0; remove: 0), the
/// primary staying put for the last three - WinForms' behavior.
fn space(rects: &[Rect], primary: usize, command: FormatCommand, horizontal: bool) -> Vec<Rect> {
    use FormatCommand::*;
    let start = |r: &Rect| if horizontal { r.left } else { r.top };
    let end = |r: &Rect| if horizontal { r.right } else { r.bottom };
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|a, b| start(&rects[*a]).total_cmp(&start(&rects[*b])).then(a.cmp(b)));
    let sorted: Vec<Rect> = order.iter().map(|i| rects[*i]).collect();
    let n = sorted.len();
    let gaps: Vec<f32> = (0..n - 1).map(|k| start(&sorted[k + 1]) - end(&sorted[k])).collect();
    let (new_gaps, anchor): (Vec<f32>, usize) = match command {
        HorizontalSpacingEqual | VerticalSpacingEqual => {
            let span = end(&sorted[n - 1]) - start(&sorted[0]);
            let sizes: f32 = sorted.iter().map(|r| end(r) - start(r)).sum();
            (vec![(span - sizes) / (n - 1) as f32; n - 1], 0)
        }
        HorizontalSpacingIncrease | VerticalSpacingIncrease => {
            (gaps.iter().map(|g| g + SPACING_STEP).collect(), order.iter().position(|i| *i == primary).unwrap_or(0))
        }
        HorizontalSpacingDecrease | VerticalSpacingDecrease => {
            (gaps.iter().map(|g| (g - SPACING_STEP).max(0.0)).collect(), order.iter().position(|i| *i == primary).unwrap_or(0))
        }
        _ => (vec![0.0; n - 1], order.iter().position(|i| *i == primary).unwrap_or(0)),
    };

    // New start of each sorted member, walking out from the anchor.
    let mut starts: Vec<f32> = sorted.iter().map(start).collect();
    for k in anchor + 1..n {
        let prev_end = starts[k - 1] + (end(&sorted[k - 1]) - start(&sorted[k - 1]));
        starts[k] = prev_end + new_gaps[k - 1];
    }
    for k in (0..anchor).rev() {
        let size = end(&sorted[k]) - start(&sorted[k]);
        starts[k] = starts[k + 1] - new_gaps[k] - size;
    }

    let mut out = rects.to_vec();
    for (k, index) in order.iter().enumerate() {
        let r = sorted[k];
        let delta = starts[k] - start(&r);
        out[*index] = if horizontal { move_rect(r, delta, 0.0) } else { move_rect(r, 0.0, delta) };
    }
    out
}

/// Centering: the members of each container, as a group, centered in that container.
fn center(members: &[FormatMember], horizontal: bool) -> Vec<Rect> {
    let mut out: Vec<Rect> = members.iter().map(|m| m.bounds).collect();
    let mut done = vec![false; members.len()];
    for i in 0..members.len() {
        if done[i] {
            continue;
        }
        let parent = members[i].parent_bounds;
        let group: Vec<usize> = (i..members.len())
            .filter(|j| !done[*j] && same_rect(members[*j].parent_bounds, parent))
            .collect();
        let Some(union) = union_rect(group.iter().map(|j| members[*j].bounds)) else { continue };
        let delta = if horizontal {
            (parent.left + parent.right) / 2.0 - (union.left + union.right) / 2.0
        } else {
            (parent.top + parent.bottom) / 2.0 - (union.top + union.bottom) / 2.0
        };
        for j in group {
            done[j] = true;
            out[j] = if horizontal { move_rect(members[j].bounds, delta, 0.0) } else { move_rect(members[j].bounds, 0.0, delta) };
        }
    }
    out
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.left - b.left).abs() < 0.01 && (a.top - b.top).abs() < 0.01 && (a.right - b.right).abs() < 0.01 && (a.bottom - b.bottom).abs() < 0.01
}

/// The edits `command` makes on the current `selection` (ONE `editRequests` batch, one undo unit):
/// for every member whose bounds [`format_rects`] changed, `X`/`Y` relative to its container's box
/// and `Width`/`Height`, each only when it changed (rounded to whole DIP). Empty when the command
/// does not apply.
pub fn format_ops(layout: &LayoutMap, doc: &ast::Document, selection: &Selection, command: FormatCommand) -> Vec<EditOp> {
    let members = format_members(layout, doc, selection);
    let primary = selection.primary().and_then(|p| members.iter().position(|m| m.id == p)).unwrap_or(0);
    let rects = format_rects(&members, primary, command);
    let mut ops = Vec::new();
    for (member, new) in members.iter().zip(rects) {
        let old = member.bounds;
        let values = [
            ("X", old.left - member.parent_bounds.left, new.left - member.parent_bounds.left),
            ("Y", old.top - member.parent_bounds.top, new.top - member.parent_bounds.top),
            ("Width", old.right - old.left, new.right - new.left),
            ("Height", old.bottom - old.top, new.bottom - new.top),
        ];
        for (name, before, after) in values {
            if (after - before).abs() >= 0.5 {
                ops.push(EditOp::SetAttribute { element_id: member.id.clone(), name: name.to_string(), value: format_dip(after.round()) });
            }
        }
    }
    ops
}

// ─────────────────────────────────────────────────────────────────────────
// The design canvas: the view shown inside a Kubuno window frame at its
// design size, on a neutral canvas, resizable like a WinForms form
// (`vskubuno/docs/DESIGNER.md` §12).
// ─────────────────────────────────────────────────────────────────────────

/// The design size used when the view declares none (neither `Width`/`Height` nor
/// `DesignWidth`/`DesignHeight` on its root element).
pub const DEFAULT_DESIGN_WIDTH: f32 = 800.0;
pub const DEFAULT_DESIGN_HEIGHT: f32 = 600.0;
/// The smallest size a canvas resize produces.
pub const MIN_DESIGN_WIDTH: f32 = 120.0;
pub const MIN_DESIGN_HEIGHT: f32 = 80.0;
/// Height of the frame's title bar, above the view's own client area.
pub const FRAME_TITLE_HEIGHT: f32 = kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT;
/// Free canvas around the frame (the frame sits at this offset from the canvas' top-left corner).
pub const CANVAS_MARGIN: f32 = 24.0;
/// Size of the frame's three resize handles (right edge, bottom edge, bottom-right corner).
pub const FRAME_HANDLE_SIZE: f32 = 8.0;
/// Thickness of the canvas' own scrollbars.
pub const SCROLLBAR_SIZE: f32 = 12.0;
/// Gap between the frame and its resize handles.
const FRAME_HANDLE_GAP: f32 = 2.0;

/// The view's design size and which root attributes hold it: its real `Width`/`Height` when the
/// root element has literal numeric ones, else the design-time `DesignWidth`/`DesignHeight`
/// (`crate::registry::DESIGN_TIME_ATTRIBUTES`), else [`DEFAULT_DESIGN_WIDTH`]×[`DEFAULT_DESIGN_HEIGHT`].
/// Each axis is decided on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignSize {
    pub width: f32,
    pub height: f32,
    /// `"Width"` or `"DesignWidth"`: the attribute a canvas resize writes the width to.
    pub width_attr: &'static str,
    /// `"Height"` or `"DesignHeight"`.
    pub height_attr: &'static str,
}

/// A literal (non-binding) positive number attribute of `element`.
fn literal_size(element: &ast::Element, name: &str) -> Option<f32> {
    element.attribute(name)?.value()?.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

/// See [`DesignSize`]. `doc` is `None` before any text was loaded (the defaults apply).
pub fn design_size(doc: Option<&ast::Document>) -> DesignSize {
    let root = doc.and_then(|d| d.root_element());
    let axis = |real: &'static str, design: &'static str, default: f32| match root.as_ref() {
        Some(r) => match literal_size(r, real) {
            Some(v) => (v, real),
            None => (literal_size(r, design).unwrap_or(default), design),
        },
        None => (default, design),
    };
    let (width, width_attr) = axis("Width", "DesignWidth", DEFAULT_DESIGN_WIDTH);
    let (height, height_attr) = axis("Height", "DesignHeight", DEFAULT_DESIGN_HEIGHT);
    DesignSize { width, height, width_attr, height_attr }
}

/// The size the view DECLARES (its root's literal `Width`/`Height`, else `DesignWidth`/
/// `DesignHeight`, per axis as [`design_size`]), or `None` when it declares neither axis - what a
/// window hosting the view opens at, like a WinForms `Form` opens at its designed `Size`. An axis
/// declared on its own takes the other's default.
pub fn declared_design_size(doc: Option<&ast::Document>) -> Option<(f32, f32)> {
    let root = doc?.root_element()?;
    let declared = ["Width", "Height", "DesignWidth", "DesignHeight"].iter().any(|name| literal_size(&root, name).is_some());
    declared.then(|| {
        let size = design_size(doc);
        (size.width, size.height)
    })
}

/// The view's title for the frame's title bar: the root element's literal `Title` (a
/// `{Binding …}` is shown as-is — it is resolved only at runtime), `None` when absent.
pub fn view_title(doc: Option<&ast::Document>) -> Option<String> {
    let title = doc?.root_element()?.attribute("Title")?.value()?;
    // A `{Res …}` title shows the resource in the design-time culture (vskubuno docs/RESOURCES.md).
    let title = match crate::binding::parse_binding(title.trim()).as_ref().and_then(crate::resources::reference) {
        Some((set, key)) => match crate::resources::value_of(set, key) {
            Some(crate::binding::Value::Str(s)) => s,
            _ => title,
        },
        None => title,
    };
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// The edits a canvas resize to `width`×`height` produces (one `setAttribute` on the root per
/// axis whose rounded value changed, on the attribute [`design_size`] says holds it) — sent as
/// ONE `editRequests` batch so the host applies it as one undo unit. Empty when nothing changed.
pub fn design_size_ops(doc: &ast::Document, width: f32, height: f32) -> Vec<EditOp> {
    let current = design_size(Some(doc));
    let mut ops = Vec::new();
    for (value, now, attr) in [(width, current.width, current.width_attr), (height, current.height, current.height_attr)] {
        let value = value.round();
        if (value - now.round()).abs() >= 1.0 {
            ops.push(EditOp::SetAttribute { element_id: String::new(), name: attr.to_string(), value: format_dip(value) });
        }
    }
    ops
}

/// After a canvas resize, the edits that make every anchored child of a Dock/Anchor container
/// (`<Panel>`) keep the place the resize gave it — what the WinForms designer does when a form is
/// resized: the controls its anchors moved or stretched get their new `Location`/`Size` written.
/// Without this, the new design size would become the panels' anchoring reference and the children
/// would jump back to their old coordinates. `layout` is the frame painted AT the new size; each
/// child's `X`/`Y` (and `Width`/`Height`, when written) become its painted rect relative to its
/// container's box. Docked children are left alone. Sent in the same batch as [`design_size_ops`].
pub fn anchored_children_ops(layout: &LayoutMap, doc: &ast::Document) -> Vec<EditOp> {
    let mut ops = Vec::new();
    for entry in layout.entries().iter().filter(|e| e.layout == LayoutKind::DockAnchor) {
        let (Some(parent), Some(element)) =
            (entry.parent_id.as_deref().and_then(|p| layout.get(p)), doc.resolve_id(&entry.id))
        else {
            continue;
        };
        let docked = element.attribute("Dock").and_then(|a| a.value()).is_some_and(|d| !d.trim().is_empty() && d.trim() != "None");
        if docked {
            continue;
        }
        let b = entry.bounds;
        let values = [
            ("X", b.left - parent.bounds.left, true),
            ("Y", b.top - parent.bounds.top, true),
            ("Width", b.right - b.left, element.attribute("Width").is_some()),
            ("Height", b.bottom - b.top, element.attribute("Height").is_some()),
        ];
        for (name, value, write) in values {
            let value = value.round();
            let current = element.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
            if write && (value - current.round()).abs() >= 1.0 {
                ops.push(EditOp::SetAttribute { element_id: entry.id.clone(), name: name.to_string(), value: format_dip(value) });
            }
        }
    }
    ops
}

/// One of the frame's three resize handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameHandle {
    /// Middle of the right edge: changes the width.
    Right,
    /// Middle of the bottom edge: changes the height.
    Bottom,
    /// Bottom-right corner: changes both.
    Corner,
}

/// Where the frame of a `width`×`height` view sits on the canvas: the whole window (`outer`), its
/// title bar and the client area the view is laid out into.
#[derive(Clone, Copy)]
pub struct FrameLayout {
    pub outer: Rect,
    pub title: Rect,
    pub client: Rect,
}

impl FrameLayout {
    /// The frame at the canvas origin `(left, top)` (already offset by the scroll position).
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self::with_title(left, top, width, height, FRAME_TITLE_HEIGHT)
    }

    /// [`FrameLayout::new`] with a title band `title_height` tall (the view's `TitleBarHeight`, a
    /// tool window's slim band, `0` for a borderless window or content under the band).
    pub fn with_title(left: f32, top: f32, width: f32, height: f32, title_height: f32) -> Self {
        let outer = Rect::new(left, top, left + width, top + title_height + height);
        let title = Rect::new(left, top, left + width, top + title_height);
        let client = Rect::new(left, top + title_height, left + width, outer.bottom);
        Self { outer, title, client }
    }

    /// The three resize handles, WinForms-style: just outside the right edge, the bottom edge
    /// and the bottom-right corner.
    pub fn handles(&self) -> [(FrameHandle, Rect); 3] {
        let s = FRAME_HANDLE_SIZE;
        let x = self.outer.right + FRAME_HANDLE_GAP;
        let y = self.outer.bottom + FRAME_HANDLE_GAP;
        let cx = (self.outer.left + self.outer.right) / 2.0;
        let cy = (self.client.top + self.client.bottom) / 2.0;
        [
            (FrameHandle::Right, Rect::new(x, cy - s / 2.0, x + s, cy + s / 2.0)),
            (FrameHandle::Bottom, Rect::new(cx - s / 2.0, y, cx + s / 2.0, y + s)),
            (FrameHandle::Corner, Rect::new(x, y, x + s, y + s)),
        ]
    }

    /// The handle under `(x, y)`, with a little slack around each square (they are small).
    pub fn handle_at(&self, x: f32, y: f32) -> Option<FrameHandle> {
        const SLACK: f32 = 3.0;
        self.handles()
            .into_iter()
            .find(|(_, r)| Rect::new(r.left - SLACK, r.top - SLACK, r.right + SLACK, r.bottom + SLACK).contains(x, y))
            .map(|(h, _)| h)
    }

    /// The canvas size needed to show the whole frame, its handles and the margin around it.
    pub fn canvas_extent(width: f32, height: f32) -> (f32, f32) {
        Self::canvas_extent_titled(width, height, FRAME_TITLE_HEIGHT)
    }

    /// [`FrameLayout::canvas_extent`] for a title band `title_height` tall.
    pub fn canvas_extent_titled(width: f32, height: f32, title_height: f32) -> (f32, f32) {
        let tail = FRAME_HANDLE_GAP + FRAME_HANDLE_SIZE + CANVAS_MARGIN;
        (CANVAS_MARGIN + width + tail, CANVAS_MARGIN + title_height + height + tail)
    }
}

/// The new design size while dragging `handle` by `(dx, dy)` from `start`: whole DIP, never below
/// [`MIN_DESIGN_WIDTH`]×[`MIN_DESIGN_HEIGHT`], the axis a handle does not control unchanged.
pub fn resized_design_size(handle: FrameHandle, start: (f32, f32), dx: f32, dy: f32) -> (f32, f32) {
    let width = match handle {
        FrameHandle::Right | FrameHandle::Corner => (start.0 + dx).round().max(MIN_DESIGN_WIDTH),
        FrameHandle::Bottom => start.0,
    };
    let height = match handle {
        FrameHandle::Bottom | FrameHandle::Corner => (start.1 + dy).round().max(MIN_DESIGN_HEIGHT),
        FrameHandle::Right => start.1,
    };
    (width, height)
}

/// An in-progress canvas resize (a drag on one of the frame's handles): the size follows the
/// pointer live (the view is re-laid out every frame) and nothing is written until the release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameResize {
    pub handle: FrameHandle,
    pub start_mouse: (f32, f32),
    pub start_size: (f32, f32),
}

impl FrameResize {
    /// The live size for the pointer at `(x, y)`.
    pub fn size_at(&self, x: f32, y: f32) -> (f32, f32) {
        resized_design_size(self.handle, self.start_size, x - self.start_mouse.0, y - self.start_mouse.1)
    }
}

/// One axis of the canvas' scroll state: clamps `offset` so that a `content`-long canvas never
/// scrolls past its end in a `viewport`-long window (0 when it fits).
pub fn clamp_scroll(offset: f32, content: f32, viewport: f32) -> f32 {
    offset.clamp(0.0, (content - viewport).max(0.0))
}

/// A scrollbar's track and thumb for a `content`-long canvas shown in `track` (the bar's own
/// rectangle) at `offset`; `None` when the content fits (no scrollbar). `horizontal` says which
/// axis `track` runs along.
pub fn scrollbar_thumb(track: Rect, horizontal: bool, content: f32, viewport: f32, offset: f32) -> Option<Rect> {
    if content <= viewport || viewport <= 0.0 {
        return None;
    }
    let len = if horizontal { track.right - track.left } else { track.bottom - track.top };
    let thumb = (len * viewport / content).max(24.0).min(len);
    let travel = len - thumb;
    let pos = if content > viewport { travel * offset / (content - viewport) } else { 0.0 };
    Some(if horizontal {
        Rect::new(track.left + pos, track.top + 2.0, track.left + pos + thumb, track.bottom - 2.0)
    } else {
        Rect::new(track.left + 2.0, track.top + pos, track.right - 2.0, track.top + pos + thumb)
    })
}

/// The scroll offset for a thumb dragged by `delta` DIP along a `len`-long track from `start_offset`.
pub fn scroll_for_thumb_drag(start_offset: f32, delta: f32, len: f32, content: f32, viewport: f32) -> f32 {
    let thumb = (len * viewport / content).max(24.0).min(len);
    let travel = (len - thumb).max(1.0);
    clamp_scroll(start_offset + delta * (content - viewport) / travel, content, viewport)
}

/// The canvas behind the frame until the IDE says otherwise: a dark neutral, like the WinForms
/// designer's in a dark theme.
pub const CANVAS_BACKGROUND: u32 = 0x2B2B2B;

thread_local! {
    /// The canvas colour the IDE asked for (its theme's designer background, `setCanvasBackground`).
    static CANVAS: std::cell::Cell<u32> = const { std::cell::Cell::new(CANVAS_BACKGROUND) };
}

/// Sets the canvas colour (`0xRRGGBB`): the IDE's designer background, sent again when its theme
/// changes, so the canvas and its scroll bars follow a live switch between light and dark.
pub fn set_canvas_background(rgb: u32) {
    CANVAS.with(|c| c.set(rgb & 0xFF_FFFF));
}

/// The canvas colour in use.
pub fn canvas_background() -> u32 {
    CANVAS.with(|c| c.get())
}

/// Whether the canvas is light (its scroll bars and marks are then dark).
fn canvas_is_light() -> bool {
    let rgb = canvas_background();
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    (r * 299 + g * 587 + b * 114) / 1000 > 128
}

/// The scroll bar thumb on the canvas (hot or not), contrasting with it.
fn scrollbar_thumb_color(hot: bool) -> u32 {
    match (canvas_is_light(), hot) {
        (true, false) => 0xC2C3C9,
        (true, true) => 0x868999,
        (false, false) => 0x5C5C5C,
        (false, true) => 0x8A8A8A,
    }
}

/// Paints the canvas background over `bounds`.
pub fn paint_canvas(c: &dyn kubuno_desktop_controls::ControlCanvas, bounds: Rect) {
    c.fill_rect(&bounds, &kubuno_desktop_ui::ribbon::hex(canvas_background()));
}

/// Paints the Kubuno window frame around the view (drawn BEFORE the view itself, which is laid
/// out into `frame.client`): the window's shadow and surface, and its title band's ground. Call
/// [`paint_view_caption`] after the view for the title, the icon and the caption buttons.
pub fn paint_view_frame(
    c: &dyn kubuno_desktop_controls::ControlCanvas,
    theme: &kubuno_desktop_ui::Theme,
    frame: &FrameLayout,
    title: &str,
    selected: bool,
) {
    let style = ViewFrameStyle { title: title.to_string(), ..ViewFrameStyle::default() };
    paint_view_frame_styled(c, theme, frame, &style, selected);
    paint_view_caption(c, frame, &style);
}

/// What the designer's window frame shows of the view's `Form` properties — everything the window
/// itself will show at run time (`WindowKind`, `FormBorderStyle`, the caption buttons, `Icon`,
/// `Subtitle`, the title bar's height and colours, `CornerPreference`, `BorderColor`, the grip…),
/// so the designer and the running window are painted by the same code
/// ([`kubuno_desktop_controls::window_chrome`]).
#[derive(Clone, Default)]
pub struct ViewFrameStyle {
    pub title: String,
    /// The root element's window properties.
    pub spec: crate::window::FormSpec,
    /// The view is a user control's (`<UserControl x:Class="…">`): designed as a plain surface, with
    /// no window frame, title band nor caption buttons — like the Windows Forms UserControl designer.
    pub user_control: bool,
}

impl ViewFrameStyle {
    /// The style the root element of `doc` writes.
    pub fn read(doc: Option<&ast::Document>) -> Self {
        let root = doc.and_then(|d| d.root_element());
        Self {
            title: view_title(doc).unwrap_or_default(),
            spec: root.as_ref().map(|r| crate::window::FormSpec::read(r, None)).unwrap_or_else(crate::window::FormSpec::read_default),
            user_control: root.as_ref().and_then(|r| r.name()).is_some_and(|n| n == "UserControl"),
        }
    }

    /// The window's properties with the theme colours resolved in `theme` (a user control's: borderless).
    pub fn form(&self, theme: &kubuno_desktop_ui::Theme) -> kubuno_desktop_controls::host::FormOptions {
        let mut form = self.spec.options_in(&crate::binding::MapViewModel::default(), Some(theme));
        if self.user_control {
            form.border_style = kubuno_desktop_controls::host::FormBorderStyle::None;
            form.corner = kubuno_desktop_controls::host::CornerPreference::Default;
            form.corner_radius = None;
            form.panel = None;
            form.border_color = None;
        }
        form
    }

    /// The radius of the window's corners as it will run (normal state), in DIP: what the frame is
    /// drawn and the view clipped with.
    pub fn corner_radius(&self, theme: &kubuno_desktop_ui::Theme) -> f32 {
        self.form(theme).corner_radius()
    }

    /// Whether the window has a title band.
    pub fn caption(&self) -> bool {
        self.form(&kubuno_desktop_ui::Theme::light()).border_style.has_caption()
    }

    /// The title band's height in the designer: `0` without a band (a borderless window) or when
    /// the view extends under it.
    pub fn title_height(&self) -> f32 {
        let form = self.form(&kubuno_desktop_ui::Theme::light());
        if !form.border_style.has_caption() {
            return 0.0;
        }
        let style = form.effective_chrome();
        if style.extend_content { 0.0 } else { style.band_height() }
    }

    /// The frame's [`FrameLayout`] at `(left, top)` for a `width`×`height` page.
    pub fn layout(&self, left: f32, top: f32, width: f32, height: f32) -> FrameLayout {
        FrameLayout::with_title(left, top, width, height, self.title_height())
    }

    /// The band the view's title-bar regions are laid out in while designing
    /// (`crate::window::set_design_chrome`); `None` for a window without one.
    pub fn design_chrome(&self, theme: &kubuno_desktop_ui::Theme, frame: &FrameLayout) -> Option<crate::window::DesignChrome> {
        let form = self.form(theme);
        if !form.border_style.has_caption() {
            return None;
        }
        Some(crate::window::DesignChrome {
            style: form.effective_chrome(),
            bounds: frame.outer,
            has_icon: form.icon.is_some(),
            buttons: form.system_buttons(),
        })
    }
}

/// [`paint_view_frame`] with the view's window properties: the window's shadow, surface and
/// border (rounded at its `CornerRadius`, else at what its `CornerPreference` and its kind give it:
/// 8 DIP for a window with a title bar), and the title band's ground.
pub fn paint_view_frame_styled(
    c: &dyn kubuno_desktop_controls::ControlCanvas,
    theme: &kubuno_desktop_ui::Theme,
    frame: &FrameLayout,
    style: &ViewFrameStyle,
    selected: bool,
) {
    use kubuno_desktop_controls::window_chrome as wc;
    let form = style.form(theme);
    let radius = form.corner_radius();
    // DWM's border: the band's colour unless the view names one (a borderless window has none).
    let border = form.border_color.or_else(|| form.border_style.has_caption().then(|| form.effective_chrome().band_color(theme)));
    wc::paint_frame(c, frame.outer, radius, border);
    if let Some(chrome) = style.design_chrome(theme, frame) {
        let l = wc::layout(&chrome.style, chrome.bounds, chrome.has_icon, chrome.buttons, Default::default());
        wc::paint_band_rounded(c, &chrome.style, &l, radius);
    }
    if selected {
        c.stroke_rounded_w(&selection_frame(frame.outer), radius + SELECTION_FRAME_GAP, &theme.accent, 1.0);
    }
}

/// Paints what sits on the band, AFTER the view (so the view's title-bar regions are known, see
/// `crate::window::declared_slots`): the window's icon, its title and subtitle, and its caption
/// buttons, inert — then the resize grip of a resizable window. The same painter as the running
/// window's ([`kubuno_desktop_controls::window_chrome::paint_caption`]).
pub fn paint_view_caption(c: &dyn kubuno_desktop_controls::ControlCanvas, frame: &FrameLayout, style: &ViewFrameStyle) {
    use kubuno_desktop_controls::window_chrome as wc;
    let theme = c.theme();
    let form = style.form(theme);
    if let Some(chrome) = style.design_chrome(theme, frame) {
        let l = wc::layout(&chrome.style, chrome.bounds, chrome.has_icon, chrome.buttons, crate::window::declared_slots());
        // A glyph name or an image file (drawn by `kubuno_desktop_controls::icon_image`, SVG included).
        let glyph = form.icon.as_deref().and_then(crate::icon::resolve);
        let bitmap = if glyph.is_none() { form.icon.as_deref().and_then(|p| kubuno_desktop_controls::styled::load_image(c, p)) } else { None };
        let icon = match (glyph, bitmap.as_ref()) {
            (Some(g), _) => wc::ChromeIcon::Glyph(g),
            (None, Some(b)) => wc::ChromeIcon::Bitmap(b),
            // A designed window with an icon it cannot load yet still shows where it goes.
            (None, None) if form.icon.is_some() => wc::ChromeIcon::Glyph("AppWindow"),
            (None, None) => wc::ChromeIcon::None,
        };
        let title = if style.title.is_empty() { "Form" } else { style.title.as_str() };
        wc::paint_caption(c, &chrome.style, &l, title, icon, wc::ChromeState::default());
    }
    if form.shows_grip() {
        // Inside the window's rounded corner, as the running window places it.
        wc::paint_grip(c, kubuno_desktop_controls::host::frame::grip_bounds(frame.outer, form.corner_radius()), false);
    }
}

/// Paints the frame's three resize handles: white squares with a dark border, like WinForms'.
pub fn paint_frame_handles(c: &dyn kubuno_desktop_controls::ControlCanvas, frame: &FrameLayout, hot: Option<FrameHandle>) {
    for (handle, r) in frame.handles() {
        let fill = if Some(handle) == hot { kubuno_desktop_ui::ribbon::hex(0xCCE4F7) } else { kubuno_desktop_ui::ribbon::hex(0xFFFFFF) };
        c.fill_rect(&r, &fill);
        c.stroke_rect(&r, &kubuno_desktop_ui::ribbon::hex(0x1E1E1E));
    }
}

/// Paints the "width × height" tooltip shown while the canvas is being resized, just below and
/// right of `(x, y)` (the pointer).
pub fn paint_size_tooltip(c: &dyn kubuno_desktop_controls::ControlCanvas, x: f32, y: f32, size: (f32, f32)) {
    let label = format!("{} × {}", format_dip(size.0.round()), format_dip(size.1.round()));
    let format = &c.formats().caption;
    let w = c.measure(&label, format) + 16.0;
    let r = Rect::new(x + 14.0, y + 18.0, x + 14.0 + w, y + 18.0 + 22.0);
    c.fill_rounded(&r, 4.0, &kubuno_desktop_ui::ribbon::hex(0x3C3C3C));
    c.stroke_rounded(&r, 4.0, &kubuno_desktop_ui::ribbon::hex(0x707070));
    c.text(&label, &Rect::new(r.left + 8.0, r.top + 3.0, r.right, r.bottom), format, &kubuno_desktop_ui::ribbon::hex(0xF1F1F1), false);
}

/// Paints one scrollbar (its track, and its thumb when the content overflows).
pub fn paint_scrollbar(c: &dyn kubuno_desktop_controls::ControlCanvas, track: Rect, thumb: Option<Rect>, hot: bool) {
    c.fill_rect(&track, &kubuno_desktop_ui::ribbon::hex(canvas_background()));
    if let Some(thumb) = thumb {
        let color = scrollbar_thumb_color(hot);
        c.fill_rounded(&thumb, 3.0, &kubuno_desktop_ui::ribbon::hex(color));
    }
}

/// What a right-click (or the keyboard context-menu key) on the surface targets.
///
/// `element_id` is `Some(id)` for an element of the view (the root element included, `""`) and
/// `None` for the view itself (the canvas background or the frame's title bar).
pub fn context_target(layout: &LayoutMap, frame: &FrameLayout, x: f32, y: f32) -> Option<String> {
    if frame.client.contains(x, y) {
        layout.hit_test(x, y).map(|e| e.id.clone())
    } else {
        // A control the view placed in the title band (`TitleBar.Region`), else the view itself.
        layout.hit_test(x, y).filter(|e| !e.id.is_empty() && frame.outer.contains(x, y)).map(|e| e.id.clone())
    }
}

/// A design-surface keyboard command the host carries out (it needs the clipboard or the
/// language server): Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+D while the surface has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DesignCommand {
    Copy,
    Cut,
    Paste,
    Duplicate,
}

/// A nudged coordinate as the plain literal text an `X`/`Y` attribute
/// already uses elsewhere in this crate (`compile.rs`'s own worked examples
/// write `Width="320"`, not `"320.0"`) — an integer DIP prints without a
/// decimal point, anything else keeps one.
fn format_dip(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
#[path = "design_outline_tests.rs"]
mod outline_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AstNode;
    use crate::syntax::parse;

    fn entry(id: &str, parent: Option<&str>, bounds: Rect, layout: LayoutKind) -> LayoutEntry {
        LayoutEntry { id: id.to_string(), parent_id: parent.map(str::to_string), bounds, layout, container: false, locked: false, clip: None }
    }

    /// A container entry (it accepts children: a press on its empty area starts a marquee).
    fn container(id: &str, parent: Option<&str>, bounds: Rect, layout: LayoutKind) -> LayoutEntry {
        LayoutEntry { container: true, ..entry(id, parent, bounds, layout) }
    }

    // ── The window frame's corners ──────────────────────────────────────

    #[test]
    fn the_designer_draws_the_window_at_its_corner_radius() {
        use kubuno_desktop_ui::graphics::testing::RecordingCanvas;
        let frame_of = |xml: &str| {
            let doc = crate::ast::Document::cast(parse(xml).syntax());
            let style = ViewFrameStyle::read(doc.as_ref());
            let canvas = RecordingCanvas::new();
            let frame = style.layout(10.0, 10.0, 400.0, 300.0);
            paint_view_frame_styled(&canvas, &kubuno_desktop_ui::Theme::light(), &frame, &style, false);
            (style.corner_radius(&kubuno_desktop_ui::Theme::light()), canvas.calls())
        };
        let (radius, calls) = frame_of(r#"<Panel Title="A"/>"#);
        assert_eq!(radius, 8.0, "rounded by default, like the running window");
        assert!(calls.iter().any(|c| c.starts_with("fill_rounded(") && c.ends_with("r=8)")), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("fill_top_rounded(") && c.ends_with("r=8)")), "the band rounds its top corners: {calls:?}");
        let (radius, calls) = frame_of(r#"<Panel Title="A" CornerRadius="24"/>"#);
        assert_eq!(radius, 24.0);
        assert!(calls.iter().any(|c| c.starts_with("fill_rounded(") && c.ends_with("r=24)")), "{calls:?}");
        let (radius, calls) = frame_of(r#"<Panel Title="A" CornerRadius="0"/>"#);
        assert_eq!(radius, 0.0);
        assert!(!calls.iter().any(|c| c.starts_with("fill_top_rounded(")), "square: {calls:?}");
        assert_eq!(frame_of(r#"<UserControl/>"#).0, 0.0, "a user control has no window frame");
    }

    // ── parent_id_of ────────────────────────────────────────────────────

    #[test]
    fn parent_id_of_matches_the_documented_examples() {
        assert_eq!(parent_id_of(""), None);
        assert_eq!(parent_id_of("3"), Some(String::new()));
        assert_eq!(parent_id_of("2.0.3"), Some("2.0".to_string()));
    }

    // ── LayoutMap / hit_test ────────────────────────────────────────────

    #[test]
    fn hit_test_prefers_the_deepest_last_pushed_match() {
        let mut map = LayoutMap::new();
        // Parent pushed first (as `DesignSlot::paint` always does before
        // recursing), a child nested inside it pushed second.
        map.push(entry("0", None, Rect::new(0.0, 0.0, 200.0, 200.0), LayoutKind::None));
        map.push(entry("0.0", Some("0"), Rect::new(10.0, 10.0, 60.0, 40.0), LayoutKind::Flow));

        let hit = map.hit_test(20.0, 20.0).expect("inside both rects");
        assert_eq!(hit.id, "0.0", "the child, not the parent, is the deepest match");

        let hit_parent_only = map.hit_test(150.0, 150.0).expect("inside the parent only");
        assert_eq!(hit_parent_only.id, "0");
    }

    #[test]
    fn hit_test_misses_outside_every_bounds() {
        let mut map = LayoutMap::new();
        map.push(entry("0", None, Rect::new(0.0, 0.0, 100.0, 100.0), LayoutKind::None));
        assert!(map.hit_test(500.0, 500.0).is_none());
    }

    #[test]
    fn hit_test_skips_degenerate_zero_area_entries() {
        let mut map = LayoutMap::new();
        map.push(entry("0", None, Rect::new(0.0, 0.0, 100.0, 100.0), LayoutKind::None));
        // A zero-width element pushed on top must not steal the hit.
        map.push(entry("0.0", Some("0"), Rect::new(10.0, 10.0, 10.0, 40.0), LayoutKind::Flow));
        let hit = map.hit_test(10.0, 20.0).expect("still inside the parent");
        assert_eq!(hit.id, "0");
    }

    #[test]
    fn get_finds_by_id_and_is_none_for_an_unknown_one() {
        let mut map = LayoutMap::new();
        map.push(entry("0", None, Rect::new(0.0, 0.0, 10.0, 10.0), LayoutKind::None));
        assert!(map.get("0").is_some());
        assert!(map.get("nope").is_none());
    }

    // `DesignSlot::paint` itself (the wrapping that turns "compiled a tree"
    // into "recorded a LayoutMap") is exercised where it can actually run:
    // `crate::compile`'s own tests confirm every element — including a
    // `<Panel>` child, the `LayoutKind::DockAnchor` case — builds through the
    // updated `build_node` without panicking; a live paint pass needs a real
    // `kubuno_desktop_controls::ControlCanvas` (Direct2D/DirectWrite), which nothing
    // in this crate's test suite constructs (see `crate::node`'s own tests'
    // doc for why) — exactly the "without a Canvas where possible" carve-out,
    // covered instead by the visual check (this package's `view_embed.rs`).

    // ── adorner geometry ────────────────────────────────────────────────

    #[test]
    fn resize_handles_are_centered_on_the_eight_compass_points() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 50.0);
        let handles = resize_handles(bounds, 8.0);
        // N (index 0): centered on the top-middle.
        assert!((handles[0].left - 46.0).abs() < 0.001);
        assert!((handles[0].top - (-4.0)).abs() < 0.001);
        // SE (index 3): centered on the bottom-right corner.
        assert!((handles[3].left - 96.0).abs() < 0.001);
        assert!((handles[3].top - 46.0).abs() < 0.001);
        // Every handle is an 8×8 square.
        for h in &handles {
            assert!((h.right - h.left - 8.0).abs() < 0.001);
            assert!((h.bottom - h.top - 8.0).abs() < 0.001);
        }
    }

    #[test]
    fn dashed_outline_covers_all_four_edges() {
        let bounds = Rect::new(0.0, 0.0, 40.0, 20.0);
        let dashes = dashed_outline(bounds, 4.0, 3.0, 1.0);
        assert!(!dashes.is_empty());
        // At least one dash touches each edge.
        assert!(dashes.iter().any(|r| r.top <= 0.001));
        assert!(dashes.iter().any(|r| r.bottom >= 19.999));
        assert!(dashes.iter().any(|r| r.left <= 0.001));
        assert!(dashes.iter().any(|r| r.right >= 39.999));
    }

    #[test]
    fn dashed_outline_of_a_degenerate_rect_is_empty() {
        assert!(dashed_outline(Rect::new(0.0, 0.0, 0.0, 10.0), 4.0, 3.0, 1.0).is_empty());
    }

    #[test]
    fn dotted_outline_covers_all_four_edges_with_one_pixel_dots() {
        let bounds = Rect::new(0.0, 0.0, 10.0, 6.0);
        let dots = dotted_outline(bounds, 1.0);
        assert!(!dots.is_empty());
        assert!(dots.iter().any(|r| r.top <= 0.001));
        assert!(dots.iter().any(|r| r.bottom >= 5.999));
        assert!(dots.iter().any(|r| r.left <= 0.001));
        assert!(dots.iter().any(|r| r.right >= 9.999));
        // Every dot is exactly one device pixel square at scale 1.0 (DIP == device pixel).
        for dot in &dots {
            assert!((dot.right - dot.left - 1.0).abs() < 0.001);
            assert!((dot.bottom - dot.top - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn dotted_outline_is_crisp_one_device_pixel_at_fractional_dpi_scale() {
        // 175% DPI: scale = 1.75. Every dot must still be exactly ONE device
        // pixel wide/tall once converted back to DIP (1.0 / 1.75), never a
        // blurred/anti-aliased fraction of a pixel — the whole point of
        // walking the perimeter in device pixels rather than DIPs.
        let scale = 1.75_f32;
        let bounds = Rect::new(0.3, 0.3, 20.7, 12.1); // deliberately not pixel-aligned
        let dots = dotted_outline(bounds, scale);
        assert!(!dots.is_empty());
        for dot in &dots {
            let w_px = (dot.right - dot.left) * scale;
            let h_px = (dot.bottom - dot.top) * scale;
            assert!((w_px - 1.0).abs() < 0.01, "dot width {w_px} device px, expected 1");
            assert!((h_px - 1.0).abs() < 0.01, "dot height {h_px} device px, expected 1");
            // Left/top edges land on the device pixel grid (integer once scaled).
            let left_px = dot.left * scale;
            assert!((left_px - left_px.round()).abs() < 0.01);
        }
    }

    #[test]
    fn dotted_outline_of_a_degenerate_rect_is_empty() {
        assert!(dotted_outline(Rect::new(0.0, 0.0, 0.0, 10.0), 1.0).is_empty());
    }

    // ── DesignController: selection ────────────────────────────────────

    fn sample_map() -> LayoutMap {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::DockAnchor));
        map.push(entry("0.0", Some("0"), Rect::new(10.0, 10.0, 110.0, 60.0), LayoutKind::DockAnchor));
        map
    }

    #[test]
    fn click_select_picks_the_deepest_element_and_reports_a_change() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        assert!(dc.click_select(&map, 50.0, 30.0));
        assert_eq!(dc.selected(), Some("0.0"));
        // Clicking the same spot again is not a change.
        assert!(!dc.click_select(&map, 50.0, 30.0));
    }

    #[test]
    fn click_select_on_empty_space_clears_the_selection() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.click_select(&map, 50.0, 30.0);
        assert!(dc.click_select(&map, 9999.0, 9999.0));
        assert_eq!(dc.selected(), None);
    }

    #[test]
    fn disabled_controller_ignores_clicks_and_hover() {
        let map = sample_map();
        let mut dc = DesignController::new();
        assert!(!dc.click_select(&map, 50.0, 30.0));
        assert_eq!(dc.selected(), None);
        dc.update_hover(&map, 50.0, 30.0);
        assert_eq!(dc.hover(), None);
    }

    #[test]
    fn escape_selects_the_parent_with_no_edit_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        let ops = dc.handle_keys(&map, None, DesignKeyInput { escape: true, ..Default::default() });
        assert!(ops.is_empty());
        assert_eq!(dc.selected(), Some("0"));
    }

    #[test]
    fn delete_requests_removal_of_the_selected_element() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        let ops = dc.handle_keys(&map, None, DesignKeyInput { delete: true, ..Default::default() });
        assert_eq!(ops, vec![EditOp::RemoveElement { element_id: "0.0".to_string() }]);
    }

    #[test]
    fn arrow_nudges_an_anchor_child_by_one_dip() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));

        // `sample_map`'s selected id "0.0" is a grandchild of the root: the
        // `<Panel>` here must be nested the same two levels deep for
        // `resolve_id("0.0")` to land on the `<Button>`.
        let p = parse(r#"<Stack><Panel><Button X="10" Y="20"/></Panel></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let ops = dc.handle_keys(&map, Some(&doc), DesignKeyInput { right: true, ..Default::default() });
        assert_eq!(ops, vec![EditOp::SetAttribute { element_id: "0.0".to_string(), name: "X".to_string(), value: "11".to_string() }]);
    }

    #[test]
    fn shift_arrow_nudges_by_a_larger_step() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));

        let p = parse(r#"<Stack><Panel><Button X="10" Y="20"/></Panel></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let ops = dc.handle_keys(&map, Some(&doc), DesignKeyInput { down: true, shift: true, ..Default::default() });
        assert_eq!(ops, vec![EditOp::SetAttribute { element_id: "0.0".to_string(), name: "Y".to_string(), value: "30".to_string() }]);
    }

    #[test]
    fn nudge_on_a_flow_child_is_a_no_op() {
        let mut map = LayoutMap::new();
        map.push(entry("0", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(entry("0.0", Some("0"), Rect::new(0.0, 0.0, 100.0, 30.0), LayoutKind::Flow));
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));

        let p = parse(r#"<Stack><Button/></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let ops = dc.handle_keys(&map, Some(&doc), DesignKeyInput { right: true, ..Default::default() });
        assert!(ops.is_empty(), "a Stack child has no X/Y to nudge");
    }

    #[test]
    fn a_locked_element_is_selected_but_never_moved_resized_or_nudged() {
        let mut map = LayoutMap::new();
        map.push(container("0", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(LayoutEntry { locked: true, ..entry("0.0", Some("0"), Rect::new(10.0, 10.0, 110.0, 40.0), LayoutKind::DockAnchor) });
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        assert!(dc.press(&map, 50.0, 20.0), "a click selects it");
        assert_eq!(dc.selected(), Some("0.0"));
        assert!(!dc.is_dragging(), "no move drag is armed");
        dc.press(&map, 110.0 + SELECTION_FRAME_GAP, 40.0 + SELECTION_FRAME_GAP);
        assert!(dc.drag.is_none(), "its handles do not resize it");
        dc.set_selected(Some("0.0".to_string()));
        let p = parse(r#"<Panel><Button X="10" Y="10" Locked="true"/></Panel>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        assert!(dc.handle_keys(&map, Some(&doc), DesignKeyInput { right: true, ..Default::default() }).is_empty());
    }

    #[test]
    fn nudge_with_no_doc_is_a_no_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        let ops = dc.handle_keys(&map, None, DesignKeyInput { right: true, ..Default::default() });
        assert!(ops.is_empty());
    }

    #[test]
    fn no_relevant_key_is_a_no_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        let ops = dc.handle_keys(&map, None, DesignKeyInput::default());
        assert!(ops.is_empty());
    }

    #[test]
    fn nothing_selected_is_always_a_no_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        let ops = dc.handle_keys(&map, None, DesignKeyInput { delete: true, ..Default::default() });
        assert!(ops.is_empty());
    }

    // ── DSG-9: resize-handle hit-test ──────────────────────────────────

    #[test]
    fn handle_at_hits_each_compass_point_and_misses_the_middle() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(handle_at(bounds, 8.0, 50.0, 0.0), Some(Handle::N));
        assert_eq!(handle_at(bounds, 8.0, 100.0, 0.0), Some(Handle::NE));
        assert_eq!(handle_at(bounds, 8.0, 100.0, 25.0), Some(Handle::E));
        assert_eq!(handle_at(bounds, 8.0, 100.0, 50.0), Some(Handle::SE));
        assert_eq!(handle_at(bounds, 8.0, 50.0, 50.0), Some(Handle::S));
        assert_eq!(handle_at(bounds, 8.0, 0.0, 50.0), Some(Handle::SW));
        assert_eq!(handle_at(bounds, 8.0, 0.0, 25.0), Some(Handle::W));
        assert_eq!(handle_at(bounds, 8.0, 0.0, 0.0), Some(Handle::NW));
        assert_eq!(handle_at(bounds, 8.0, 50.0, 25.0), None, "the middle of the box is not a handle");
    }

    // ── DSG-9: move/resize geometry ────────────────────────────────────

    #[test]
    fn move_rect_translates_without_resizing() {
        let r = move_rect(Rect::new(10.0, 10.0, 60.0, 40.0), 5.0, -3.0);
        assert!((r.left - 15.0).abs() < 0.001);
        assert!((r.top - 7.0).abs() < 0.001);
        assert!((r.right - 65.0).abs() < 0.001);
        assert!((r.bottom - 37.0).abs() < 0.001);
    }

    #[test]
    fn resize_rect_e_only_grows_the_right_edge() {
        let r = resize_rect(Rect::new(0.0, 0.0, 100.0, 50.0), Handle::E, 20.0, 999.0);
        assert!((r.left - 0.0).abs() < 0.001);
        assert!((r.top - 0.0).abs() < 0.001);
        assert!((r.right - 120.0).abs() < 0.001);
        assert!((r.bottom - 50.0).abs() < 0.001);
    }

    #[test]
    fn resize_rect_w_moves_the_left_edge_and_keeps_the_right_fixed() {
        let r = resize_rect(Rect::new(10.0, 0.0, 100.0, 50.0), Handle::W, -5.0, 0.0);
        assert!((r.left - 5.0).abs() < 0.001);
        assert!((r.right - 100.0).abs() < 0.001);
    }

    #[test]
    fn resize_rect_clamps_at_the_minimum_element_size() {
        let r = resize_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Handle::E, -1000.0, 0.0);
        assert!((r.right - r.left - MIN_ELEMENT_SIZE).abs() < 0.001);
        let r = resize_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Handle::W, 1000.0, 0.0);
        assert!((r.right - r.left - MIN_ELEMENT_SIZE).abs() < 0.001);
    }

    // ── DSG-9: snapping ─────────────────────────────────────────────────

    #[test]
    fn snap_bounds_snaps_a_close_edge_and_reports_the_guide() {
        let dragged = Rect::new(52.0, 10.0, 152.0, 60.0);
        let sibling = Rect::new(0.0, 0.0, 50.0, 100.0);
        let (snapped, guides) = snap_bounds(dragged, &[sibling], 6.0);
        assert!((snapped.left - 50.0).abs() < 0.001);
        assert!((snapped.right - 150.0).abs() < 0.001, "width is preserved");
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].axis, SnapAxis::Vertical);
        assert!((guides[0].at - 50.0).abs() < 0.001);
    }

    #[test]
    fn snap_bounds_does_nothing_beyond_the_threshold() {
        let dragged = Rect::new(80.0, 10.0, 180.0, 60.0);
        let sibling = Rect::new(0.0, 0.0, 50.0, 100.0);
        let (snapped, guides) = snap_bounds(dragged, &[sibling], 6.0);
        assert!((snapped.left - 80.0).abs() < 0.001);
        assert!(guides.is_empty());
    }

    #[test]
    fn snap_bounds_snaps_independently_on_each_axis() {
        // Close on Y (top edges within threshold), far on X.
        let dragged = Rect::new(200.0, 3.0, 250.0, 33.0);
        let sibling = Rect::new(0.0, 0.0, 50.0, 30.0);
        let (snapped, guides) = snap_bounds(dragged, &[sibling], 6.0);
        assert!((snapped.left - 200.0).abs() < 0.001, "X did not snap");
        assert!((snapped.top - 0.0).abs() < 0.001, "Y snapped to the sibling's own top edge");
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].axis, SnapAxis::Horizontal);
    }

    // ── DSG-9: flow axis / insertion index / marker ────────────────────

    #[test]
    fn flow_axis_infers_vertical_for_a_stacked_column() {
        let siblings = [Rect::new(0.0, 0.0, 100.0, 30.0), Rect::new(0.0, 30.0, 100.0, 60.0)];
        assert_eq!(flow_axis(&siblings), FlowAxis::Vertical);
    }

    #[test]
    fn flow_axis_infers_horizontal_for_a_row() {
        let siblings = [Rect::new(0.0, 0.0, 50.0, 30.0), Rect::new(50.0, 0.0, 100.0, 30.0)];
        assert_eq!(flow_axis(&siblings), FlowAxis::Horizontal);
    }

    #[test]
    fn flow_axis_defaults_to_vertical_with_fewer_than_two_siblings() {
        assert_eq!(flow_axis(&[]), FlowAxis::Vertical);
        assert_eq!(flow_axis(&[Rect::new(0.0, 0.0, 10.0, 10.0)]), FlowAxis::Vertical);
    }

    #[test]
    fn flow_insertion_index_finds_the_right_slot() {
        let siblings = [
            Rect::new(0.0, 0.0, 100.0, 30.0),
            Rect::new(0.0, 30.0, 100.0, 60.0),
            Rect::new(0.0, 60.0, 100.0, 90.0),
        ];
        assert_eq!(flow_insertion_index(&siblings, 0.0, 5.0), 0, "above the first block");
        assert_eq!(flow_insertion_index(&siblings, 0.0, 45.0), 1, "between the first and second");
        assert_eq!(flow_insertion_index(&siblings, 0.0, 85.0), 3, "past the last block");
    }

    #[test]
    fn flow_insertion_index_of_an_empty_container_is_zero() {
        assert_eq!(flow_insertion_index(&[], 10.0, 10.0), 0);
    }

    #[test]
    fn flow_insertion_marker_before_the_first_sits_on_its_own_edge() {
        let siblings = [Rect::new(0.0, 10.0, 100.0, 40.0), Rect::new(0.0, 40.0, 100.0, 70.0)];
        let container = Rect::new(0.0, 0.0, 100.0, 100.0);
        let m = flow_insertion_marker(&siblings, container, 0, 4.0);
        assert!((m.top - 8.0).abs() < 0.001);
        assert!((m.bottom - 12.0).abs() < 0.001);
    }

    #[test]
    fn flow_insertion_marker_between_siblings_sits_at_the_midpoint_gap() {
        let siblings = [Rect::new(0.0, 0.0, 100.0, 40.0), Rect::new(0.0, 50.0, 100.0, 90.0)];
        let container = Rect::new(0.0, 0.0, 100.0, 100.0);
        let m = flow_insertion_marker(&siblings, container, 1, 4.0);
        assert!((m.top - 43.0).abs() < 0.001);
        assert!((m.bottom - 47.0).abs() < 0.001);
    }

    #[test]
    fn flow_insertion_marker_past_the_end_uses_the_last_edge() {
        let siblings = [Rect::new(0.0, 0.0, 100.0, 40.0)];
        let container = Rect::new(0.0, 0.0, 100.0, 100.0);
        let m = flow_insertion_marker(&siblings, container, 5, 4.0);
        assert!((m.top - 38.0).abs() < 0.001);
    }

    // ── DSG-9: DesignController drag state machine ─────────────────────

    #[test]
    fn press_on_an_unselected_element_selects_it_and_arms_a_pending_drag() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        let changed = dc.press(&map, 50.0, 30.0);
        assert!(changed, "selection changed from nothing to \"0.0\"");
        assert_eq!(dc.selected(), Some("0.0"));
        // Armed in the SAME press (`DESIGNER.md` DSG-9's press-and-drag fix)
        // — but not yet confirmed, so no preview until the pointer moves.
        assert!(dc.is_dragging());
        assert!(dc.drag_preview().is_none());
    }

    #[test]
    fn press_on_the_already_selected_anchor_child_arms_a_pending_move_drag() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        let changed = dc.press(&map, 50.0, 30.0);
        assert!(!changed, "it was already selected");
        assert!(dc.is_dragging());
        assert!(dc.drag_preview().is_none(), "armed but not yet past DRAG_THRESHOLD");
    }

    #[test]
    fn a_press_release_with_no_real_movement_never_confirms_the_drag() {
        // The press-and-drag fix must not turn an ordinary click (which
        // happens to land on a draggable element) into a spurious 1-DIP nudge.
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.press(&map, 50.0, 30.0);
        dc.update_drag(&map, 51.0, 31.0, false); // well under DRAG_THRESHOLD
        assert!(dc.drag_preview().is_none());
        assert_eq!(dc.end_drag(None), DragOutcome::None);
    }

    #[test]
    fn move_drag_updates_the_preview_and_end_drag_emits_a_batched_move() {
        let map = sample_map(); // "0.0": (10,10)-(110,60), DockAnchor
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 50.0, 30.0);

        dc.update_drag(&map, 70.0, 30.0, true); // dx=+20 (past DRAG_THRESHOLD), shift suppresses snap
        let preview = dc.drag_preview().expect("a Move drag has a live preview");
        assert!((preview.left - 30.0).abs() < 0.001);
        assert!(dc.drag_guides().is_empty(), "snap suppressed by shift");

        let p = parse(r#"<Stack><Panel><Button X="10" Y="20"/></Panel></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        match dc.end_drag(Some(&doc)) {
            DragOutcome::Batch { ops, gesture } => {
                assert_eq!(gesture, Gesture::Move);
                assert_eq!(ops, vec![EditOp::SetAttribute { element_id: "0.0".to_string(), name: "X".to_string(), value: "30".to_string() }]);
            }
            other => panic!("expected a Move batch, got {other:?}"),
        }
        assert!(!dc.is_dragging());
        assert!(dc.drag_preview().is_none());
    }

    #[test]
    fn move_drag_with_zero_movement_ends_as_a_no_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 50.0, 30.0);
        dc.update_drag(&map, 50.0, 30.0, true);

        let p = parse(r#"<Stack><Panel><Button X="10" Y="20"/></Panel></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        assert_eq!(dc.end_drag(Some(&doc)), DragOutcome::None);
    }

    #[test]
    fn resize_drag_from_the_se_handle_changes_width_and_height() {
        let map = sample_map(); // "0.0": (10,10)-(110,60) -> width 100, height 50
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 110.0, 60.0); // the SE handle sits at the bottom-right corner
        assert!(dc.is_dragging(), "a handle press arms a CONFIRMED resize immediately, no threshold");

        dc.update_drag(&map, 130.0, 80.0, true); // +20 width, +20 height

        let p = parse(r#"<Stack><Panel><Button X="10" Y="10" Width="100" Height="50"/></Panel></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        match dc.end_drag(Some(&doc)) {
            DragOutcome::Batch { ops, gesture } => {
                assert_eq!(gesture, Gesture::Resize);
                assert!(ops.contains(&EditOp::SetAttribute { element_id: "0.0".to_string(), name: "Width".to_string(), value: "120".to_string() }));
                assert!(ops.contains(&EditOp::SetAttribute { element_id: "0.0".to_string(), name: "Height".to_string(), value: "70".to_string() }));
            }
            other => panic!("expected a Resize batch, got {other:?}"),
        }
    }

    #[test]
    fn esc_during_a_drag_cancels_it_without_an_edit_op() {
        let map = sample_map();
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 50.0, 30.0);
        dc.update_drag(&map, 90.0, 30.0, true);
        assert!(dc.is_dragging());

        let ops = dc.handle_keys(&map, None, DesignKeyInput { escape: true, ..Default::default() });
        assert!(ops.is_empty());
        assert!(!dc.is_dragging());
        // Cancelling a drag is not the ordinary Esc-to-parent: the selection
        // itself is untouched.
        assert_eq!(dc.selected(), Some("0.0"));
    }

    #[test]
    fn press_on_a_selected_flow_child_arms_a_reorder_drag_and_drop_emits_move_element() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 100.0, 300.0), LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(0.0, 0.0, 100.0, 90.0), LayoutKind::None));
        map.push(entry("0.0", Some("0"), Rect::new(0.0, 0.0, 100.0, 30.0), LayoutKind::Flow));
        map.push(entry("0.1", Some("0"), Rect::new(0.0, 30.0, 100.0, 60.0), LayoutKind::Flow));
        map.push(entry("0.2", Some("0"), Rect::new(0.0, 60.0, 100.0, 90.0), LayoutKind::Flow));

        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 50.0, 15.0);
        assert!(dc.is_dragging());

        dc.update_drag(&map, 50.0, 90.0, false); // drag down past both remaining siblings' midpoints (and DRAG_THRESHOLD)
        assert!(dc.reorder_marker(&map).is_some());

        match dc.end_drag(None) {
            DragOutcome::Single(EditOp::MoveElement { element_id, new_parent_id, index }) => {
                assert_eq!(element_id, "0.0");
                assert_eq!(new_parent_id, "0");
                assert_eq!(index, 2);
            }
            other => panic!("expected a MoveElement, got {other:?}"),
        }
        assert!(!dc.is_dragging());
    }

    #[test]
    fn reorder_drag_back_to_the_original_slot_ends_as_a_no_op() {
        let mut map = LayoutMap::new();
        map.push(entry("0", None, Rect::new(0.0, 0.0, 100.0, 90.0), LayoutKind::None));
        map.push(entry("0.0", Some("0"), Rect::new(0.0, 0.0, 100.0, 30.0), LayoutKind::Flow));
        map.push(entry("0.1", Some("0"), Rect::new(0.0, 30.0, 100.0, 60.0), LayoutKind::Flow));

        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc.set_selected(Some("0.0".to_string()));
        dc.press(&map, 50.0, 15.0);
        dc.update_drag(&map, 50.0, 16.0, false); // barely moved - never even crosses DRAG_THRESHOLD
        assert_eq!(dc.end_drag(None), DragOutcome::None);
    }

    // ── DSG-9: toolbox drop ─────────────────────────────────────────────

    #[cfg(feature = "family-containers")]
    #[test]
    fn toolbox_drop_into_an_empty_anchor_container_carries_xy() {
        // `<Panel/>` is itself the document's ROOT element (stable id `""`) —
        // there is no separate wrapper node to give a child id.
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));

        let p = parse(r#"<Panel/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("Button".to_string());
        let target = tb.drag_over(&map, &doc, 40.0, 60.0).cloned().expect("a Panel is a valid drop container");
        assert_eq!(target.parent_id, "");
        assert!(target.valid);
        assert_eq!(target.xy, Some((40.0, 60.0)));

        let op = tb.drop().expect("a valid target drops");
        match op {
            EditOp::InsertChild { parent_id, index, xml } => {
                assert_eq!(parent_id, "");
                assert_eq!(index, 0);
                // At the drop point, with the Button's default size and WinForms' default anchoring.
                assert_eq!(xml, r#"<Button X="40" Y="60" Width="100" Height="36" Anchor="Top, Left"/>"#);
            }
            other => panic!("expected InsertChild, got {other:?}"),
        }
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn toolbox_drop_into_an_anchor_container_is_whole_dip_relative_to_the_container() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(10.0, 20.0, 410.0, 320.0), LayoutKind::None));
        let p = parse(r#"<Panel/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("TextField".to_string());
        let target = tb.drag_over(&map, &doc, 50.4, 80.6).cloned().unwrap();
        assert_eq!(target.xy, Some((40.0, 61.0)));
        // The ghost is the element's real future rect.
        let m = target.marker;
        assert_eq!((m.left, m.top, m.right, m.bottom), (50.0, 81.0, 250.0, 117.0));
        let Some(EditOp::InsertChild { xml, .. }) = tb.drop() else { panic!("expected InsertChild") };
        assert_eq!(xml, r#"<TextField X="40" Y="61" Width="200" Height="36" Anchor="Top, Left"/>"#);
    }

    #[test]
    fn default_drop_sizes_follow_the_control_kind() {
        assert_eq!(default_drop_size("Button"), (100.0, 36.0));
        assert_eq!(default_drop_size("TextField"), (200.0, 36.0));
        assert_eq!(default_drop_size("CheckBox"), (140.0, 24.0));
        assert_eq!(default_drop_size("NotAComponent"), (120.0, 36.0));
    }

    #[test]
    fn toolbox_drop_into_an_empty_flow_container_has_no_xy() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));

        let p = parse(r#"<Stack/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("Button".to_string());
        let target = tb.drag_over(&map, &doc, 40.0, 60.0).cloned().expect("a Stack is a valid drop container");
        assert!(target.valid);
        assert_eq!(target.xy, None);

        let op = tb.drop().unwrap();
        assert_eq!(op, EditOp::InsertChild { parent_id: String::new(), index: 0, xml: "<Button/>".to_string() });
    }

    #[test]
    fn toolbox_drop_routes_a_leaf_hit_to_its_own_parent() {
        // `<Stack>` is the root (id `""`), `<Button>` is its first (and
        // only) child (id `"0"`).
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(0.0, 0.0, 100.0, 30.0), LayoutKind::Flow));

        let p = parse(r#"<Stack><Button/></Stack>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("Switch".to_string());
        // Point lands inside the existing `<Button>` leaf ("0"), below its
        // own midpoint — which does not itself accept children, so the
        // target must be its parent Stack, inserted AFTER the Button.
        let target = tb.drag_over(&map, &doc, 10.0, 25.0).cloned().unwrap();
        assert_eq!(target.parent_id, "");
        assert_eq!(target.index, 1, "inserted after the existing Button");
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn toolbox_drop_of_a_gated_child_outside_its_required_parent_is_invalid() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));

        let p = parse(r#"<Stack/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("TabItem".to_string()); // only valid directly under <Tabs>
        let target = tb.drag_over(&map, &doc, 10.0, 10.0).cloned().unwrap();
        assert!(!target.valid);
        assert!(tb.drop().is_none(), "an invalid target never drops");
    }

    #[test]
    fn toolbox_drop_into_an_already_full_single_widget_container_is_invalid() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));

        let p = parse(r#"<Card><Button/></Card>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("Switch".to_string());
        let target = tb.drag_over(&map, &doc, 10.0, 10.0).cloned().unwrap();
        assert!(!target.valid, "Card already has its one allowed child");
    }

    #[test]
    fn toolbox_drop_of_an_unknown_component_is_invalid() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));

        let p = parse(r#"<Stack/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("NotAComponent".to_string());
        let target = tb.drag_over(&map, &doc, 10.0, 10.0).cloned().unwrap();
        assert!(!target.valid);
    }

    #[test]
    fn drag_leave_clears_the_toolbox_drag() {
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        let p = parse(r#"<Stack/>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();

        let mut tb = ToolboxController::new();
        tb.drag_enter("Button".to_string());
        assert!(tb.is_active());
        tb.drag_leave();
        assert!(!tb.is_active());
        assert!(tb.drag_over(&map, &doc, 10.0, 10.0).is_none());
        assert!(tb.drop().is_none());
    }

    // ── design canvas ───────────────────────────────────────────────────

    fn doc_of(src: &str) -> ast::Document {
        ast::Document::cast(parse(src).syntax()).unwrap()
    }

    #[test]
    fn design_size_defaults_when_the_view_declares_none() {
        let doc = doc_of("<Card><Stack/></Card>");
        let size = design_size(Some(&doc));
        assert_eq!((size.width, size.height), (DEFAULT_DESIGN_WIDTH, DEFAULT_DESIGN_HEIGHT));
        assert_eq!((size.width_attr, size.height_attr), ("DesignWidth", "DesignHeight"));
        assert_eq!(design_size(None).width, DEFAULT_DESIGN_WIDTH);
    }

    #[test]
    fn design_size_prefers_real_width_and_height_per_axis() {
        let doc = doc_of(r#"<Card Width="400" DesignWidth="999" DesignHeight="300"/>"#);
        let size = design_size(Some(&doc));
        assert_eq!((size.width, size.width_attr), (400.0, "Width"));
        assert_eq!((size.height, size.height_attr), (300.0, "DesignHeight"));
    }

    #[test]
    fn a_bound_width_is_not_a_design_size() {
        let doc = doc_of(r#"<Card Width="{Binding W}"/>"#);
        let size = design_size(Some(&doc));
        assert_eq!((size.width, size.width_attr), (DEFAULT_DESIGN_WIDTH, "DesignWidth"));
    }

    #[test]
    fn design_size_ops_write_the_attribute_holding_each_axis() {
        let doc = doc_of(r#"<Card Height="300"><Stack/></Card>"#);
        let ops = design_size_ops(&doc, 640.4, 480.0);
        assert_eq!(
            ops,
            vec![
                EditOp::SetAttribute { element_id: String::new(), name: "DesignWidth".into(), value: "640".into() },
                EditOp::SetAttribute { element_id: String::new(), name: "Height".into(), value: "480".into() },
            ]
        );
        assert!(design_size_ops(&doc, 800.0, 300.0).is_empty());
    }

    #[test]
    fn declared_design_size_is_none_unless_the_view_declares_one() {
        assert_eq!(declared_design_size(Some(&doc_of("<Panel/>"))), None);
        assert_eq!(declared_design_size(None), None);
        assert_eq!(declared_design_size(Some(&doc_of(r#"<Panel DesignWidth="640" DesignHeight="360"/>"#))), Some((640.0, 360.0)));
        assert_eq!(
            declared_design_size(Some(&doc_of(r#"<Panel DesignWidth="640"/>"#))),
            Some((640.0, DEFAULT_DESIGN_HEIGHT))
        );
    }

    #[test]
    fn view_title_is_the_root_title() {
        assert_eq!(view_title(Some(&doc_of(r#"<Card Title=" Réglages "/>"#))).as_deref(), Some("Réglages"));
        assert_eq!(view_title(Some(&doc_of("<Card/>"))), None);
    }

    #[test]
    fn frame_layout_puts_the_client_area_under_the_title_bar() {
        let f = FrameLayout::new(10.0, 20.0, 300.0, 200.0);
        assert_eq!((f.client.left, f.client.top, f.client.right, f.client.bottom), (10.0, 70.0, 310.0, 270.0));
        assert_eq!(f.title.bottom, f.client.top);
    }

    #[test]
    fn frame_handles_sit_on_the_right_bottom_and_corner() {
        let f = FrameLayout::new(0.0, 0.0, 300.0, 200.0);
        let right = f.handles()[0].1;
        assert!(right.left > f.outer.right);
        assert!(((right.top + right.bottom) / 2.0 - (f.client.top + f.client.bottom) / 2.0).abs() < 0.01);
        assert_eq!(f.handle_at(f.outer.right + 5.0, f.outer.bottom + 5.0), Some(FrameHandle::Corner));
        assert_eq!(f.handle_at(150.0, f.outer.bottom + 5.0), Some(FrameHandle::Bottom));
        assert_eq!(f.handle_at(f.outer.right + 5.0, f.client.top + 100.0), Some(FrameHandle::Right));
        assert_eq!(f.handle_at(150.0, 100.0), None);
    }

    #[test]
    fn resizing_follows_the_handle_axis_rounds_and_clamps() {
        assert_eq!(resized_design_size(FrameHandle::Right, (300.0, 200.0), 50.4, 99.0), (350.0, 200.0));
        assert_eq!(resized_design_size(FrameHandle::Bottom, (300.0, 200.0), 50.0, -30.6), (300.0, 169.0));
        assert_eq!(
            resized_design_size(FrameHandle::Corner, (300.0, 200.0), -1000.0, -1000.0),
            (MIN_DESIGN_WIDTH, MIN_DESIGN_HEIGHT)
        );
        let drag = FrameResize { handle: FrameHandle::Corner, start_mouse: (10.0, 10.0), start_size: (300.0, 200.0) };
        assert_eq!(drag.size_at(30.0, 40.0), (320.0, 230.0));
    }

    #[test]
    fn scroll_offsets_are_clamped_to_the_overflow() {
        assert_eq!(clamp_scroll(50.0, 100.0, 200.0), 0.0);
        assert_eq!(clamp_scroll(500.0, 1000.0, 400.0), 500.0);
        assert_eq!(clamp_scroll(900.0, 1000.0, 400.0), 600.0);
        assert_eq!(clamp_scroll(-5.0, 1000.0, 400.0), 0.0);
    }

    #[test]
    fn scrollbar_thumb_only_when_the_content_overflows() {
        let track = Rect::new(0.0, 0.0, 12.0, 400.0);
        assert!(scrollbar_thumb(track, false, 300.0, 400.0, 0.0).is_none());
        let top = scrollbar_thumb(track, false, 800.0, 400.0, 0.0).unwrap();
        assert_eq!((top.top, top.bottom), (0.0, 200.0));
        let bottom = scrollbar_thumb(track, false, 800.0, 400.0, 400.0).unwrap();
        assert_eq!((bottom.top, bottom.bottom), (200.0, 400.0));
        assert_eq!(scroll_for_thumb_drag(0.0, 100.0, 400.0, 800.0, 400.0), 200.0);
    }

    #[test]
    fn context_target_is_the_element_inside_the_client_area_else_the_view() {
        let frame = FrameLayout::new(0.0, 0.0, 300.0, 200.0);
        let mut map = LayoutMap::new();
        map.push(entry("", None, frame.client, LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(10.0, 50.0, 100.0, 80.0), LayoutKind::Flow));
        assert_eq!(context_target(&map, &frame, 20.0, 60.0).as_deref(), Some("0"));
        assert_eq!(context_target(&map, &frame, 200.0, 150.0).as_deref(), Some(""));
        assert_eq!(context_target(&map, &frame, 20.0, 10.0), None); // The title bar.
        assert_eq!(context_target(&map, &frame, 500.0, 500.0), None); // The canvas.
    }

    #[test]
    fn anchored_children_keep_their_resized_place_but_docked_ones_are_left_alone() {
        let doc = doc_of(r#"<Panel><Button X="300" Y="10" Width="80" Height="24" Anchor="Top, Right"/><TextField Dock="Top" Height="30"/><Label X="5" Y="5"/></Panel>"#);
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(10.0, 50.0, 610.0, 450.0), LayoutKind::None));
        // Painted after the panel grew by 200: the button moved right, the label stayed.
        map.push(entry("0", Some(""), Rect::new(510.0, 60.0, 590.0, 84.0), LayoutKind::DockAnchor));
        map.push(entry("1", Some(""), Rect::new(10.0, 50.0, 610.0, 80.0), LayoutKind::DockAnchor));
        map.push(entry("2", Some(""), Rect::new(15.0, 55.0, 45.0, 70.0), LayoutKind::DockAnchor));
        assert_eq!(
            anchored_children_ops(&map, &doc),
            vec![EditOp::SetAttribute { element_id: "0".into(), name: "X".into(), value: "500".into() }]
        );
    }

    #[test]
    fn escape_keeps_the_view_selected() {
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        let mut map = LayoutMap::new();
        map.push(entry("", None, Rect::new(0.0, 0.0, 100.0, 100.0), LayoutKind::None));
        dc.set_selected(Some(String::new()));
        dc.handle_keys(&map, None, DesignKeyInput { escape: true, ..Default::default() });
        assert_eq!(dc.selected(), Some(""));
    }

    // ── §13: multi-selection ────────────────────────────────────────────

    /// The view's root `<Panel>` ("" - a container) holding three Anchor buttons.
    fn multi_map() -> LayoutMap {
        let mut map = LayoutMap::new();
        map.push(container("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(10.0, 10.0, 60.0, 40.0), LayoutKind::DockAnchor));
        map.push(entry("1", Some(""), Rect::new(100.0, 20.0, 160.0, 50.0), LayoutKind::DockAnchor));
        map.push(entry("2", Some(""), Rect::new(200.0, 100.0, 260.0, 130.0), LayoutKind::DockAnchor));
        map
    }

    const MULTI_DOC: &str = r#"<Panel><Button X="10" Y="10" Width="50" Height="30"/><Button X="100" Y="20" Width="60" Height="30"/><Button X="200" Y="100" Width="60" Height="30"/></Panel>"#;

    fn enabled() -> DesignController {
        let mut dc = DesignController::new();
        dc.set_enabled(true);
        dc
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn set(name: &str, value: &str, id: &str) -> EditOp {
        EditOp::SetAttribute { element_id: id.to_string(), name: name.to_string(), value: value.to_string() }
    }

    const CTRL: PointerModifiers = PointerModifiers { ctrl: true, shift: false };
    const SHIFT: PointerModifiers = PointerModifiers { ctrl: false, shift: true };

    #[test]
    fn selection_toggle_add_and_primary_follow_winforms() {
        let mut s = Selection::single("0");
        s.add("1".to_string());
        assert_eq!(s.ids(), ids(&["0", "1"]).as_slice());
        assert_eq!(s.primary(), Some("1"), "the last added is the primary");
        s.toggle("2".to_string());
        assert_eq!(s.primary(), Some("2"));
        s.toggle("2".to_string());
        assert_eq!(s.ids(), ids(&["0", "1"]).as_slice());
        assert_eq!(s.primary(), Some("1"), "removing the primary makes the last one left primary");
        assert!(s.make_primary("0"));
        assert!(!s.make_primary("9"));
        assert_eq!(s.primary(), Some("0"));
    }

    #[test]
    fn selection_never_mixes_the_view_with_its_elements() {
        let mut s = Selection::single("");
        s.add("0".to_string());
        assert_eq!(s.ids(), ids(&["0"]).as_slice(), "adding to the view replaces it");
        s.add(String::new());
        assert_eq!(s.ids(), ids(&[""]).as_slice(), "adding the view replaces the rest");
        s.set_many(ids(&["1", "", "1", "2"]), Some("9".to_string()));
        assert_eq!(s.ids(), ids(&["1", "2"]).as_slice());
        assert_eq!(s.primary(), Some("1"), "an unknown primary falls back to the first");
    }

    #[test]
    fn top_level_ids_drops_the_root_and_nested_selections() {
        assert_eq!(top_level_ids(&ids(&["", "0", "0.1", "1.0", "10"])), ids(&["0", "1.0", "10"]));
        assert!(is_ancestor_id("", "0") && is_ancestor_id("1", "1.0") && !is_ancestor_id("1", "10") && !is_ancestor_id("1", "1"));
    }

    #[test]
    fn marquee_hits_the_touched_children_of_its_container_only() {
        let mut map = multi_map();
        map.push(entry("1.0", Some("1"), Rect::new(110.0, 25.0, 120.0, 35.0), LayoutKind::DockAnchor));
        let rect = marquee_rect((170.0, 0.0), (50.0, 45.0)); // normalized: (50,0)-(170,45)
        assert!((rect.left - 50.0).abs() < 0.001 && (rect.bottom - 45.0).abs() < 0.001);
        assert_eq!(marquee_hits(&map, "", rect), ids(&["0", "1"]), "\"2\" is untouched, \"1.0\" is not a child of the view");
    }

    #[test]
    fn a_drag_on_the_view_background_is_a_marquee_selecting_what_it_touches() {
        let map = multi_map();
        let mut dc = enabled();
        dc.press(&map, 300.0, 5.0); // the root Panel's empty area
        assert_eq!(dc.selected(), Some(""), "a press on a container's empty area selects it");
        assert!(dc.is_dragging());
        dc.update_drag(&map, 50.0, 45.0, false);
        assert!(dc.marquee_rect().is_some());
        assert_eq!(dc.marquee_hits(), ids(&["0", "1"]).as_slice());
        assert_eq!(dc.end_drag(None), DragOutcome::None, "a marquee never edits");
        assert_eq!(dc.selection().ids(), ids(&["0", "1"]).as_slice());
        assert_eq!(dc.selected(), Some("0"));
        assert!(!dc.is_dragging());
    }

    #[test]
    fn a_click_on_a_container_background_just_selects_the_container() {
        let map = multi_map();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), None);
        dc.press(&map, 300.0, 5.0);
        dc.update_drag(&map, 301.0, 6.0, false); // under DRAG_THRESHOLD
        assert!(dc.marquee_rect().is_none());
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&[""]).as_slice());
    }

    #[test]
    fn ctrl_marquee_toggles_and_escape_cancels_a_marquee() {
        let map = multi_map();
        let mut dc = enabled();
        dc.set_selection(ids(&["2"]), None);
        dc.press_with(&map, 300.0, 5.0, CTRL);
        assert_eq!(dc.selection().ids(), ids(&["2"]).as_slice(), "a Ctrl marquee does not select the container");
        dc.update_drag(&map, 5.0, 140.0, false); // touches all three
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["0", "1"]).as_slice(), "\"2\" toggled off, \"0\"/\"1\" on");

        dc.press_with(&map, 300.0, 5.0, SHIFT);
        dc.update_drag(&map, 250.0, 120.0, false);
        let ops = dc.handle_keys(&map, None, DesignKeyInput { escape: true, ..Default::default() });
        assert!(ops.is_empty());
        assert!(!dc.is_dragging());
        assert_eq!(dc.selection().ids(), ids(&["0", "1"]).as_slice(), "Esc leaves the selection as it was");
    }

    #[test]
    fn ctrl_click_toggles_shift_click_adds_and_a_click_keeps_the_multi_selection() {
        let map = multi_map();
        let mut dc = enabled();
        dc.press(&map, 20.0, 20.0);
        dc.end_drag(None);
        dc.press_with(&map, 120.0, 30.0, CTRL);
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["0", "1"]).as_slice());
        dc.press_with(&map, 220.0, 110.0, SHIFT);
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["0", "1", "2"]).as_slice());
        assert_eq!(dc.selected(), Some("2"));
        dc.press_with(&map, 120.0, 30.0, CTRL);
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["0", "2"]).as_slice());
        // A plain click on a selected element keeps the multi-selection and makes it primary.
        dc.press(&map, 20.0, 20.0);
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["0", "2"]).as_slice());
        assert_eq!(dc.selected(), Some("0"));
        // A plain click elsewhere selects that element alone.
        dc.press(&map, 120.0, 30.0);
        dc.end_drag(None);
        assert_eq!(dc.selection().ids(), ids(&["1"]).as_slice());
    }

    #[test]
    fn a_nested_container_is_marqueed_until_selected_then_dragged() {
        let mut map = LayoutMap::new();
        map.push(container("", None, Rect::new(0.0, 0.0, 400.0, 300.0), LayoutKind::None));
        map.push(container("0", Some(""), Rect::new(10.0, 10.0, 200.0, 200.0), LayoutKind::DockAnchor));
        let mut dc = enabled();
        dc.press(&map, 100.0, 100.0);
        assert_eq!(dc.selected(), Some("0"));
        assert!(dc.marquee_rect().is_none() && dc.drag_preview().is_none());
        dc.update_drag(&map, 150.0, 150.0, true);
        assert!(dc.marquee_rect().is_some(), "an unselected container's background starts a marquee");
        dc.end_drag(None);
        dc.press(&map, 100.0, 100.0);
        dc.update_drag(&map, 150.0, 150.0, true);
        assert!(dc.drag_preview().is_some(), "a selected container is moved");
    }

    #[test]
    fn a_group_move_moves_every_selected_anchor_child() {
        let map = multi_map();
        let p = parse(MULTI_DOC);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), Some("0".to_string()));
        dc.press(&map, 120.0, 30.0); // on "1": stays a multi-selection, "1" becomes primary
        assert_eq!(dc.selection().len(), 2);
        dc.update_drag(&map, 140.0, 30.0, true);
        assert_eq!(dc.drag_previews().len(), 2);
        match dc.end_drag(Some(&doc)) {
            DragOutcome::Batch { ops, gesture } => {
                assert_eq!(gesture, Gesture::Move);
                assert_eq!(ops, vec![set("X", "30", "0"), set("X", "120", "1")]);
            }
            other => panic!("expected a Move batch, got {other:?}"),
        }
    }

    #[test]
    fn a_group_move_snaps_the_group_bounds_to_the_unselected_siblings() {
        let map = multi_map();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), Some("0".to_string()));
        dc.press(&map, 20.0, 20.0);
        // The group spans (10,10)-(160,50); +38 puts its right edge at 198, 2 DIP from "2"'s left (200).
        dc.update_drag(&map, 58.0, 20.0, false);
        let guides = dc.drag_guides();
        assert!(guides.iter().any(|g| g.axis == SnapAxis::Vertical && (g.at - 200.0).abs() < 0.001), "snapped to x=200");
        let previews = dc.drag_previews();
        assert!((previews[1].right - 200.0).abs() < 0.001);
        assert!((previews[0].left - 50.0).abs() < 0.001, "every member moved by the snapped delta");
    }

    #[test]
    fn a_group_resize_applies_the_primary_handle_delta_to_every_member() {
        let map = multi_map();
        let p = parse(MULTI_DOC);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), Some("1".to_string()));
        // "1"'s selection frame is (97,17)-(163,53): its SE handle is centred on (163,53).
        dc.press(&map, 163.0, 53.0);
        dc.update_drag(&map, 173.0, 63.0, true);
        match dc.end_drag(Some(&doc)) {
            DragOutcome::Batch { ops, gesture } => {
                assert_eq!(gesture, Gesture::Resize);
                assert_eq!(ops, vec![set("Width", "60", "0"), set("Height", "40", "0"), set("Width", "70", "1"), set("Height", "40", "1")]);
            }
            other => panic!("expected a Resize batch, got {other:?}"),
        }
    }

    #[test]
    fn apply_edge_deltas_never_goes_below_the_minimum_size() {
        let r = apply_edge_deltas(Rect::new(0.0, 0.0, 10.0, 10.0), (0.0, 0.0, -50.0, 0.0));
        assert!((r.right - MIN_ELEMENT_SIZE).abs() < 0.001 && r.left.abs() < 0.001);
        let r = apply_edge_deltas(Rect::new(0.0, 0.0, 10.0, 10.0), (0.0, 50.0, 0.0, 0.0));
        assert!((r.top - (10.0 - MIN_ELEMENT_SIZE)).abs() < 0.001 && (r.bottom - 10.0).abs() < 0.001);
    }

    #[test]
    fn ctrl_a_selects_the_primary_siblings() {
        let map = multi_map();
        let mut dc = enabled();
        dc.set_selected(Some("1".to_string()));
        dc.handle_keys(&map, None, DesignKeyInput { select_all: true, ..Default::default() });
        assert_eq!(dc.selection().ids(), ids(&["0", "1", "2"]).as_slice());
        assert_eq!(dc.selected(), Some("1"), "the primary stays");
        dc.set_selected(Some(String::new()));
        dc.handle_keys(&map, None, DesignKeyInput { select_all: true, ..Default::default() });
        assert_eq!(dc.selection().ids(), ids(&["0", "1", "2"]).as_slice(), "the view selected: its top-level elements");
    }

    #[test]
    fn delete_and_nudge_apply_to_the_whole_selection() {
        let map = multi_map();
        let p = parse(MULTI_DOC);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "2"]), None);
        let ops = dc.handle_keys(&map, Some(&doc), DesignKeyInput { right: true, ..Default::default() });
        assert_eq!(ops, vec![set("X", "11", "0"), set("X", "201", "2")]);
        let ops = dc.handle_keys(&map, Some(&doc), DesignKeyInput { delete: true, ..Default::default() });
        assert_eq!(ops, vec![EditOp::RemoveElement { element_id: "0".into() }, EditOp::RemoveElement { element_id: "2".into() }]);
    }

    #[test]
    fn a_right_click_on_a_selected_element_keeps_the_multi_selection() {
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), Some("0".to_string()));
        dc.select_for_context("1".to_string());
        assert_eq!(dc.selection().ids(), ids(&["0", "1"]).as_slice());
        assert_eq!(dc.selected(), Some("1"));
        dc.select_for_context("2".to_string());
        assert_eq!(dc.selection().ids(), ids(&["2"]).as_slice());
    }

    // ── §13: format commands ────────────────────────────────────────────

    fn member(id: &str, bounds: Rect) -> FormatMember {
        FormatMember { id: id.to_string(), bounds, parent_bounds: Rect::new(0.0, 0.0, 400.0, 300.0) }
    }

    fn lefts(rects: &[Rect]) -> Vec<f32> {
        rects.iter().map(|r| r.left).collect()
    }

    fn tops(rects: &[Rect]) -> Vec<f32> {
        rects.iter().map(|r| r.top).collect()
    }

    fn members3() -> Vec<FormatMember> {
        vec![
            member("0", Rect::new(10.0, 10.0, 60.0, 40.0)),     // w 50
            member("1", Rect::new(100.0, 20.0, 160.0, 50.0)),   // w 60
            member("2", Rect::new(300.0, 100.0, 340.0, 120.0)), // w 40, h 20
        ]
    }

    #[test]
    fn align_is_relative_to_the_primary() {
        let m = members3();
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::AlignLefts)), vec![100.0, 100.0, 100.0]);
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::AlignRights)), vec![110.0, 100.0, 120.0]);
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::AlignCenters)), vec![105.0, 100.0, 110.0]);
        assert_eq!(tops(&format_rects(&m, 0, FormatCommand::AlignBottoms)), vec![10.0, 10.0, 20.0]);
        assert_eq!(tops(&format_rects(&m, 0, FormatCommand::AlignMiddles)), vec![10.0, 10.0, 15.0]);
        assert_eq!(tops(&format_rects(&m, 2, FormatCommand::AlignTops)), vec![100.0, 100.0, 100.0]);
    }

    #[test]
    fn make_same_size_copies_the_primary_size() {
        let m = members3();
        let r = format_rects(&m, 0, FormatCommand::MakeSameSize);
        assert!(r.iter().all(|r| (r.right - r.left - 50.0).abs() < 0.001 && (r.bottom - r.top - 30.0).abs() < 0.001));
        let r = format_rects(&m, 2, FormatCommand::MakeSameWidth);
        assert!((r[1].right - r[1].left - 40.0).abs() < 0.001 && (r[1].bottom - r[1].top - 30.0).abs() < 0.001);
        let r = format_rects(&m, 2, FormatCommand::MakeSameHeight);
        assert!((r[0].bottom - r[0].top - 20.0).abs() < 0.001 && (r[0].right - r[0].left - 50.0).abs() < 0.001);
    }

    #[test]
    fn spacing_equal_keeps_the_outer_members_and_evens_the_gaps() {
        let m = members3();
        // Span 10..340 = 330, widths 150: two gaps of 90.
        assert_eq!(lefts(&format_rects(&m, 0, FormatCommand::HorizontalSpacingEqual)), vec![10.0, 150.0, 300.0]);
        assert_eq!(lefts(&format_rects(&m[..2], 0, FormatCommand::HorizontalSpacingEqual)), vec![10.0, 100.0], "needs three");
    }

    #[test]
    fn spacing_increase_decrease_and_remove_keep_the_primary_in_place() {
        let m = members3();
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::HorizontalSpacingIncrease)), vec![2.0, 100.0, 308.0]);
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::HorizontalSpacingDecrease)), vec![18.0, 100.0, 292.0]);
        assert_eq!(lefts(&format_rects(&m, 1, FormatCommand::HorizontalSpacingRemove)), vec![50.0, 100.0, 160.0]);
        assert_eq!(tops(&format_rects(&m, 0, FormatCommand::VerticalSpacingRemove)), vec![10.0, 40.0, 70.0]);
    }

    #[test]
    fn center_moves_the_group_to_the_middle_of_its_container() {
        let m = vec![member("0", Rect::new(10.0, 10.0, 60.0, 40.0)), member("1", Rect::new(100.0, 20.0, 150.0, 50.0))];
        // Group 10..150 (centre 80) in 0..400 (centre 200): +120.
        assert_eq!(lefts(&format_rects(&m, 0, FormatCommand::CenterHorizontally)), vec![130.0, 220.0]);
        assert_eq!(tops(&format_rects(&m[..1], 0, FormatCommand::CenterVertically)), vec![135.0], "a single element is centred too");
    }

    #[test]
    fn format_ops_write_parent_relative_values_and_skip_docked_elements() {
        let mut map = LayoutMap::new();
        map.push(container("", None, Rect::new(20.0, 20.0, 420.0, 320.0), LayoutKind::None));
        map.push(entry("0", Some(""), Rect::new(30.0, 30.0, 80.0, 60.0), LayoutKind::DockAnchor));
        map.push(entry("1", Some(""), Rect::new(120.0, 40.0, 180.0, 70.0), LayoutKind::DockAnchor));
        map.push(entry("2", Some(""), Rect::new(20.0, 20.0, 420.0, 50.0), LayoutKind::DockAnchor));
        let p = parse(r#"<Panel><Button X="10" Y="10"/><Button X="100" Y="20"/><Button Dock="Top"/></Panel>"#);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let mut selection = Selection::new();
        selection.set_many(ids(&["0", "1", "2"]), Some("0".to_string()));
        assert_eq!(format_members(&map, &doc, &selection).len(), 2, "the docked element is left out");
        assert_eq!(format_ops(&map, &doc, &selection, FormatCommand::AlignTops), vec![set("Y", "10", "1")]);
        assert!(format_ops(&map, &doc, &selection, FormatCommand::HorizontalSpacingEqual).is_empty(), "two members only");
        assert_eq!(format_min_members(FormatCommand::CenterVertically), 1);
    }

    #[test]
    fn a_shift_press_on_the_primary_handle_still_resizes() {
        let map = multi_map();
        let mut dc = enabled();
        dc.set_selection(ids(&["0", "1"]), Some("1".to_string()));
        dc.press_with(&map, 163.0, 53.0, SHIFT); // Shift = no snapping, not "add to the selection"
        dc.update_drag(&map, 173.0, 63.0, true);
        assert_eq!(dc.selection().len(), 2);
        assert_eq!(dc.drag_previews().len(), 2, "a group resize");
    }

    #[test]
    fn a_drag_writes_whole_dip_values() {
        let map = multi_map();
        let p = parse(MULTI_DOC);
        let doc = ast::Document::cast(p.syntax()).unwrap();
        let mut dc = enabled();
        dc.set_selected(Some("0".to_string()));
        dc.press(&map, 20.0, 20.0);
        dc.update_drag(&map, 30.4, 20.3, true); // a pointer delta at 150 %: (10.4, 0.3)
        match dc.end_drag(Some(&doc)) {
            DragOutcome::Batch { ops, .. } => assert_eq!(ops, vec![set("X", "20", "0")], "rounded, and Y (0.3) is no change"),
            other => panic!("expected a Move batch, got {other:?}"),
        }
    }
}
