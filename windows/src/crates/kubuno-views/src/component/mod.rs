//! # The control hierarchy (WinForms model)
//!
//! Work package EVT-7a of `vskubuno/docs/EVENTS.md`: the cascading hierarchy rooted in
//! WinForms' `System.ComponentModel.Component` → `System.Windows.Forms.Control`, and the
//! overridable `On…` methods, in idiomatic Rust — **a trait per level, with default methods (the
//! "virtual" behaviour), an embedded base-state struct per level, and generated delegation**:
//!
//! ```text
//! Component                (ComponentCore)            site, DesignMode, Dispose / Disposed
//! └─ Control               (ControlCore)              bounds, visible/enabled, focus, styles,
//!    │                                                 every on_… override, events, invalidate
//!    ├─ ButtonBase         (ButtonBaseCore)           Button, IconButton, CheckBox, RadioButton, Switch
//!    ├─ TextBoxBase        (TextBoxBaseCore)          TextField, TextArea, MaskedField, SearchField
//!    ├─ ListControl        (ListControlCore)          ListBox, CheckedListBox, ComboBox, Dropdown
//!    ├─ LabelBase          (LabelBaseCore)            Label, LinkLabel, Badge
//!    ├─ RangeBase          (RangeBaseCore)            Slider, ProgressBar, NumericField
//!    └─ ScrollableControl  (ScrollableControlCore)    ScrollArea
//!       ├─ ContainerBase   (ContainerBaseCore)        Panel, GroupBox, Card, Stack, Tabs, Splitter, Accordion
//!       └─ ContainerControl (ContainerControlCore)
//!          ├─ UserControl  (UserControlCore)          composite controls (EVT-7b)
//!          └─ View         (ViewCore)                 the Form: Load, Shown, FormClosing…
//! ```
//!
//! (the other controls — `Icon`, `ListView`, `DataTable`, `DatePicker`… — derive `Control`
//! directly; the structural elements `<TabItem>`, `<Item>`… are non-visual `Component`s. See
//! [`crate::controls`] for the whole list; `ScrollBarBase` of the design note is `RangeBase`, the
//! base of every value-in-a-range control.)
//!
//! ## Writing a control
//!
//! ```
//! use kubuno_views::prelude::*;
//!
//! /// A button drawn as a pill, which counts its clicks.
//! #[derive(Component)]
//! #[kubuno(extends = Button, overrides(Control))]
//! struct RoundButton {
//!     base: Button,
//!     clicks: u32,
//! }
//!
//! impl Control for RoundButton {
//!     fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
//!         let r = e.clip_rectangle;
//!         let radius = (r.bottom - r.top) / 2.0;
//!         let theme = e.graphics.theme();
//!         e.graphics.fill_rounded(&r, radius, &theme.accent);
//!         e.raise(self, "OnPaint"); // what the base's on_paint would end with
//!     }
//!
//!     fn on_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
//!         self.base_mut().on_click(e); // base.OnClick(e): raises Click
//!         self.clicks += 1;
//!     }
//! }
//!
//! let mut b = RoundButton { base: Button::new("Round"), clicks: 0 };
//! let log = std::rc::Rc::new(std::cell::Cell::new(0));
//! let seen = log.clone();
//! b.click().subscribe(move |_, _| seen.set(seen.get() + 1)).detach();
//! b.perform_click();
//! assert_eq!((b.clicks, log.get()), (1, 1));
//!
//! // Upcasts (trait upcasting) and downcasts.
//! let as_button_base: &dyn ButtonBase = &b;
//! let as_control: &dyn Control = as_button_base;
//! let as_component: &dyn Component = as_control;
//! assert!(as_component.is::<RoundButton>() && as_component.is_a("ButtonBase"));
//! assert_eq!(as_component.class_chain(), ["RoundButton", "Button", "ButtonBase", "Control", "Component"]);
//! assert_eq!(as_component.downcast_ref::<RoundButton>().map(|r| r.clicks), Some(1));
//! assert!(as_component.find_base::<Button>().is_some());
//! ```
//!
//! - `#[derive(Component)]` + `#[kubuno(extends = X)]` generates the plumbing: the chain's
//!   hidden link traits (the base-state accessors and the upcasts), `base()` / `base_mut()`, the
//!   type chain, and an empty `impl` of every level trait of the chain — except those listed in
//!   `overrides(…)`, which you write yourself with the methods you override. `extends` names a
//!   built-in control class (`Button`) — the base field is then that class — or a level
//!   (`ButtonBase`, `Control`…) — the base field is then the level's core (`ButtonBaseCore`).
//! - **A method you do not override runs the base's**: every default method first delegates to
//!   the base object (`RoundButton` → its `Button`), and only the root behaviour raises the event.
//!   So overriding and subscribing compose as in WinForms: an override that calls
//!   `self.base_mut().on_click(e)` raises `Click`; one that does not suppresses it.
//! - Virtual dispatch happens on the object the host holds: the router, the nodes and
//!   [`ControlHost`] call every `on_…` on the outermost control, so `RoundButton::on_click` runs
//!   even though `Button` implements the rest. A base implementation reached by delegation calls
//!   its own methods on itself, not on the derived object — keep behaviour that must reach an
//!   override in provided trait methods called on the outer object, as `perform_click` does.
//!
//! ## Where the hierarchy runs
//!
//! Every element of a `.kbview` view is backed by an instance of its class
//! ([`crate::design::DesignSlot`] owns it): the input router (EVT-2) delivers MouseDown, KeyDown,
//! GotFocus, Validating… through the class's `on_…` methods ([`Control::dispatch_event`]), whose
//! base behaviour raises the XML handler, then the Rust subscribers; a node's own events (a
//! `<Button>`'s Click) go the same way; `<Button>` paints through its class's `on_paint`. For
//! controls built in Rust, [`ControlHost`] does the same outside any view.

pub mod control;
pub mod cx;
pub mod host;
pub mod levels;
/// Painting a control: styles, background, buffer (EVT-8).
pub mod paint;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

pub use control::{AccessiblePart, BoundsSpecified, Control, ControlCore, ControlLink, ControlStyles, CreateParams, HasControlCore, Keys, Message};
pub use cx::{EventCx, PaintEventCx, RaiseSink};
pub use host::{ControlHost, HostedControl};
pub use paint::{paint_control, PaintBuffer, PaintOutcome};
pub use levels::*;
/// The ribbon family's levels (`RibbonControl`, `RibbonItem`, `vskubuno/docs/RIBBON.md` §3).
mod ribbon_levels;
pub use ribbon_levels::*;

use crate::events::{ElementRef, EmptyEventArgs, Event, EventArgs};

/// `#[derive(Component)]` — see the macro's documentation for its `#[kubuno(…)]` options.
pub use kubuno_views_macros::Component;
/// `#[derive(UserControl)]` (EVT-7b): a composite designed as a `.kbview` of its own — see the
/// macro's documentation.
pub use kubuno_views_macros::UserControl;
/// `#[derive(PropertyValue)]` (EVT-7b): a fieldless enum as a property type.
pub use kubuno_views_macros::PropertyValue;

pub mod overrides;
mod property;
pub use property::{raise_declared_event, PropertyValue, Shared};

/// WinForms' `ISite`: the component's name and container at design time, and whether it is
/// being designed ([`Component::design_mode`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Site {
    /// The component's name in its container (`x:Name`).
    pub name: String,
    /// The component is hosted by a designer (the Visual Studio design surface).
    pub design_mode: bool,
    /// The container's name, when there is one (the view's file).
    pub container: Option<String>,
}

/// The state every component carries: its site, whether it was disposed, and its events' Rust
/// subscribers.
#[derive(Default)]
pub struct ComponentCore {
    pub site: Option<Site>,
    pub disposed: bool,
    /// The Rust subscribers of every event of the component, by attribute name.
    pub events: EventMap,
    /// Events the component raised to its element's `.kbview` handler outside an event dispatch
    /// (a declared event's `raise_…` method, EVT-7b), delivered when the element next paints.
    queued: Vec<(&'static str, Box<dyn EventArgs>)>,
}

impl ComponentCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `event` (its attribute name) for the element's `.kbview` handler: the view delivers
    /// it at the end of the element's next paint, in the same frame when raised from a handler or
    /// an override.
    pub fn queue_event(&mut self, event: &'static str, args: Box<dyn EventArgs>) {
        self.queued.push((event, args));
    }

    /// Takes the queued events (see [`Self::queue_event`]), oldest first.
    pub fn take_queued(&mut self) -> Vec<(&'static str, Box<dyn EventArgs>)> {
        std::mem::take(&mut self.queued)
    }

    /// Whether events are waiting for the element's handler.
    pub fn has_queued(&self) -> bool {
        !self.queued.is_empty()
    }
}

/// Reaches the [`ComponentCore`] of a component or of a level's core. Generated by
/// `#[derive(Component)]`; the cores implement it by hand.
#[doc(hidden)]
pub trait HasComponentCore {
    fn component_core(&self) -> &ComponentCore;
    fn component_core_mut(&mut self) -> &mut ComponentCore;
}

impl HasComponentCore for ComponentCore {
    fn component_core(&self) -> &ComponentCore {
        self
    }
    fn component_core_mut(&mut self) -> &mut ComponentCore {
        self
    }
}

/// The plumbing of a component class, generated by `#[derive(Component)]` (never written by
/// hand): the type information, the base object (for delegation), and the upcasts to every level
/// the class belongs to.
#[doc(hidden)]
pub trait ComponentLink: HasComponentCore + Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_component(&self) -> &dyn Component;
    fn as_component_mut(&mut self) -> &mut dyn Component;
    /// The class name (`"RoundButton"`).
    fn class_name(&self) -> &'static str;
    /// The class followed by its ancestors, `"Component"` last.
    fn class_chain(&self) -> &'static [&'static str];
    /// The embedded base object, when the class extends another class (not a level).
    fn base_component(&self) -> Option<&dyn Component>;
    fn base_component_mut(&mut self) -> Option<&mut dyn Component>;

    fn as_control(&self) -> Option<&dyn Control> {
        None
    }
    fn as_control_mut(&mut self) -> Option<&mut dyn Control> {
        None
    }
    fn as_scrollable_control(&self) -> Option<&dyn ScrollableControl> {
        None
    }
    fn as_scrollable_control_mut(&mut self) -> Option<&mut dyn ScrollableControl> {
        None
    }
    fn as_container_control(&self) -> Option<&dyn ContainerControl> {
        None
    }
    fn as_container_control_mut(&mut self) -> Option<&mut dyn ContainerControl> {
        None
    }
    fn as_user_control(&self) -> Option<&dyn UserControl> {
        None
    }
    fn as_user_control_mut(&mut self) -> Option<&mut dyn UserControl> {
        None
    }
    fn as_view(&self) -> Option<&dyn View> {
        None
    }
    fn as_view_mut(&mut self) -> Option<&mut dyn View> {
        None
    }
    fn as_button_base(&self) -> Option<&dyn ButtonBase> {
        None
    }
    fn as_button_base_mut(&mut self) -> Option<&mut dyn ButtonBase> {
        None
    }
    fn as_text_box_base(&self) -> Option<&dyn TextBoxBase> {
        None
    }
    fn as_text_box_base_mut(&mut self) -> Option<&mut dyn TextBoxBase> {
        None
    }
    fn as_list_control(&self) -> Option<&dyn ListControl> {
        None
    }
    fn as_list_control_mut(&mut self) -> Option<&mut dyn ListControl> {
        None
    }
    fn as_label_base(&self) -> Option<&dyn LabelBase> {
        None
    }
    fn as_label_base_mut(&mut self) -> Option<&mut dyn LabelBase> {
        None
    }
    fn as_container_base(&self) -> Option<&dyn ContainerBase> {
        None
    }
    fn as_container_base_mut(&mut self) -> Option<&mut dyn ContainerBase> {
        None
    }
    fn as_range_base(&self) -> Option<&dyn RangeBase> {
        None
    }
    fn as_range_base_mut(&mut self) -> Option<&mut dyn RangeBase> {
        None
    }
    fn as_ribbon_control(&self) -> Option<&dyn RibbonControl> {
        None
    }
    fn as_ribbon_control_mut(&mut self) -> Option<&mut dyn RibbonControl> {
        None
    }
    fn as_ribbon_item(&self) -> Option<&dyn RibbonItem> {
        None
    }
    fn as_ribbon_item_mut(&mut self) -> Option<&mut dyn RibbonItem> {
        None
    }

    /// Sets the declared property `name` (its XML attribute, EVT-7b) from `value`; `false` when
    /// the class has no such property or the value does not convert. Generated by
    /// `#[derive(Component)]` for the `#[property]` fields; the default asks the base object.
    fn kubuno_set_property(&mut self, name: &str, value: &crate::binding::Value) -> bool {
        match self.base_component_mut() {
            Some(base) => base.kubuno_set_property(name, value),
            None => false,
        }
    }

    /// The value of the declared property `name` (see [`Self::kubuno_set_property`]).
    fn kubuno_get_property(&self, name: &str) -> Option<crate::binding::Value> {
        self.base_component().and_then(|base| base.kubuno_get_property(name))
    }

    /// A user control's view model (itself, generated by `#[derive(UserControl)]`): what the
    /// bindings and handlers of its own `.kbview` run against. `None` for other classes.
    fn kubuno_view_model(&mut self) -> Option<&mut dyn crate::binding::ViewModel> {
        None
    }
}

/// The root of the hierarchy (WinForms `System.ComponentModel.Component`): anything that can sit
/// in a view — a control, or a non-visual component such as a `<TabItem>`. Implemented by
/// `#[derive(Component)]` (empty) unless the class lists `Component` in `overrides(…)`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a component class",
    label = "not a `Component`",
    note = "derive it: `#[derive(Component)] #[kubuno(extends = Button)] struct {Self} {{ base: Button, … }}`; when the class lists `Component` in `overrides(…)`, write `impl Component for {Self} {{ … }}`"
)]
pub trait Component: ComponentLink {
    /// The site (WinForms `Component.Site`): name, container, design mode.
    fn site(&self) -> Option<&Site> {
        self.component_core().site.as_ref()
    }

    /// Sites (or unsites) the component. The view runtime sites every element of a view.
    fn set_site(&mut self, site: Option<Site>) {
        self.component_core_mut().site = site;
    }

    /// WinForms `DesignMode`: the component is hosted by the designer, which renders it but
    /// never raises the application's events. A control can paint a design-time hint with it.
    fn design_mode(&self) -> bool {
        self.component_core().site.as_ref().is_some_and(|s| s.design_mode)
    }

    /// `x:Name` / `Control.Name` (the site's name, else the control's own), `""` when unnamed.
    fn display_name(&self) -> &str {
        if let Some(c) = self.as_control() {
            let name = c.control_core().name.as_str();
            if !name.is_empty() {
                return name;
            }
        }
        self.component_core().site.as_ref().map(|s| s.name.as_str()).unwrap_or("")
    }

    /// Whether [`Component::dispose`] ran.
    fn is_disposed(&self) -> bool {
        self.component_core().disposed
    }

    /// WinForms `Dispose()`: releases what the component holds ([`Component::dispose_core`]),
    /// then raises `Disposed`. Idempotent. Not meant to be overridden: override `dispose_core`.
    fn dispose(&mut self) {
        if self.component_core().disposed {
            return;
        }
        self.dispose_core(true);
        self.component_core_mut().disposed = true;
        let mut args = EmptyEventArgs;
        EventCx::new(&mut args).from_class(self.class_name()).raise(&*self, "OnDisposed");
    }

    /// WinForms `Dispose(bool disposing)`: what an override releases. The default delegates to
    /// the base object.
    fn dispose_core(&mut self, disposing: bool) {
        if let Some(base) = self.base_component_mut() {
            base.dispose_core(disposing);
        }
    }

    /// The `Disposed` event.
    fn disposed(&self) -> Event<EmptyEventArgs> {
        self.component_core().events.event("OnDisposed")
    }

    /// The data-binding face of a component (DATA-2, [`crate::scope::BindingProvider`]): a
    /// `BindingSource` answers the binding paths below its name. The default asks the base object
    /// (a class extending a provider is one).
    fn as_binding_provider(&self) -> Option<&dyn crate::scope::BindingProvider> {
        self.base_component().and_then(|b| b.as_binding_provider())
    }

    /// [`Component::as_binding_provider`], mutably.
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn crate::scope::BindingProvider> {
        self.base_component_mut().and_then(|b| b.as_binding_provider_mut())
    }
}

impl dyn Component {
    /// Whether the component is exactly a `T`.
    pub fn is<T: Component>(&self) -> bool {
        self.as_any().is::<T>()
    }

    /// Checked downcast to the concrete class.
    pub fn downcast_ref<T: Component>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Checked mutable downcast to the concrete class.
    pub fn downcast_mut<T: Component>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    /// Whether `name` is the class or one of its ancestors (`is_a("ButtonBase")`), the `is`
    /// test of the Toolbox and of tooling.
    pub fn is_a(&self, name: &str) -> bool {
        self.class_chain().contains(&name)
    }

    /// The component itself or the embedded base object of class `T` (a `RoundButton`'s
    /// `Button`): what `((Button)control)` reaches in WinForms.
    pub fn find_base<T: Component>(&self) -> Option<&T> {
        if self.as_any().is::<T>() {
            return self.as_any().downcast_ref::<T>();
        }
        self.base_component()?.find_base::<T>()
    }

    /// `find_base`, mutably.
    pub fn find_base_mut<T: Component>(&mut self) -> Option<&mut T> {
        if self.as_any().is::<T>() {
            return self.as_any_mut().downcast_mut::<T>();
        }
        self.base_component_mut()?.find_base_mut::<T>()
    }
}

/// `downcast_ref` / `is_a` on the other levels' trait objects (they upcast to `dyn Component`).
macro_rules! level_downcasts {
    ($($level:ident),*) => {$(
        impl dyn $level {
            /// Checked downcast to the concrete class (see `dyn Component::downcast_ref`).
            pub fn downcast_ref<T: Component>(&self) -> Option<&T> {
                self.as_any().downcast_ref::<T>()
            }
            /// Checked mutable downcast to the concrete class.
            pub fn downcast_mut<T: Component>(&mut self) -> Option<&mut T> {
                self.as_any_mut().downcast_mut::<T>()
            }
            /// Whether `name` is the class or one of its ancestors.
            pub fn is_a(&self, name: &str) -> bool {
                self.class_chain().contains(&name)
            }
        }
    )*};
}

level_downcasts!(Control, ScrollableControl, ContainerControl, UserControl, View, ButtonBase, TextBoxBase, ListControl, LabelBase, ContainerBase, RangeBase, RibbonControl, RibbonItem);

/// The compile-time type chain of a class or of a level's core: `["Button", "ButtonBase",
/// "Control", "Component"]`. `#[derive(Component)]` builds a class's from its base's.
pub trait Lineage {
    const CHAIN: &'static [&'static str];
}

/// The compile-time description of a component class (generated by `#[derive(Component)]`).
pub trait ClassInfo: Component + Lineage + Sized {
    /// The class name.
    const NAME: &'static str;
    /// The embedded base: a class (`Button`) or a level's core (`ButtonBaseCore`).
    type Base;
}

/// The Rust subscribers of a component's events, by attribute name (`"OnClick"`), created on
/// first use (a control nobody subscribes to allocates nothing).
///
/// ```
/// use kubuno_views::component::EventMap;
/// use kubuno_views::events::{ElementRef, EventArgs, MouseEventArgs};
///
/// let map = EventMap::default();
/// let _s = map.event::<MouseEventArgs>("OnClick").subscribe(|_, e| e.clicks += 1);
/// let mut e = MouseEventArgs::default();
/// map.raise("OnClick", &ElementRef::detached("Ok"), &mut e);
/// assert_eq!(e.clicks, 1);
/// assert!(map.has_subscribers("OnClick") && !map.has_subscribers("OnMouseDown"));
/// ```
#[derive(Default)]
pub struct EventMap {
    entries: RefCell<Vec<(&'static str, Rc<dyn ErasedEvent>)>>,
}

/// An `Event<A>` whose `A` is known only at run time.
trait ErasedEvent {
    fn as_any(&self) -> &dyn Any;
    fn raise_dyn(&self, sender: &ElementRef<'_>, args: &mut dyn EventArgs);
    fn subscribers(&self) -> usize;
}

impl<A: EventArgs> ErasedEvent for Event<A> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn raise_dyn(&self, sender: &ElementRef<'_>, args: &mut dyn EventArgs) {
        match args.downcast_mut::<A>() {
            Some(args) => self.raise(sender, args),
            None => tracing::warn!(
                expected = std::any::type_name::<A>(),
                got = args.type_chain().first().copied().unwrap_or("?"),
                "event raised with args of another type: its Rust subscribers are skipped"
            ),
        }
    }
    fn subscribers(&self) -> usize {
        self.subscriber_count()
    }
}

impl EventMap {
    /// The event named `name` (a handle: subscribe to it, raise it). The first call fixes its
    /// args type; asking again with another type logs an error and returns a detached event.
    pub fn event<A: EventArgs>(&self, name: &'static str) -> Event<A> {
        let found = self.entries.borrow().iter().find(|(n, _)| *n == name).map(|(_, e)| e.clone());
        match found {
            Some(existing) => match existing.as_any().downcast_ref::<Event<A>>() {
                Some(event) => event.clone(),
                None => {
                    tracing::error!(event = name, args = std::any::type_name::<A>(), "event asked for with another args type");
                    Event::new()
                }
            },
            None => {
                let event: Event<A> = Event::new();
                self.entries.borrow_mut().push((name, Rc::new(event.clone())));
                event
            }
        }
    }

    /// Raises `name` to its subscribers (nothing when nobody ever asked for it).
    pub fn raise(&self, name: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) {
        // Cloned out first: a handler may subscribe to another event of the same component.
        let found = self.entries.borrow().iter().find(|(n, _)| *n == name).map(|(_, e)| e.clone());
        if let Some(event) = found {
            event.raise_dyn(sender, args);
        }
    }

    /// Whether `name` has at least one subscriber.
    pub fn has_subscribers(&self, name: &str) -> bool {
        self.entries.borrow().iter().any(|(n, e)| *n == name && e.subscribers() > 0)
    }
}

#[cfg(test)]
mod tests;
