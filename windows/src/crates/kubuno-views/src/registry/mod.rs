//! The component metadata registry — `XML_VIEWS.md` §4.
//!
//! "One table per family, in the new crate, entirely additive": the data
//! here is what the loader will read to build widgets (phase 2c), what a
//! generated XSD/JSON-schema will read for VS's XML IntelliSense, what the
//! language server will read for hover/completion (phase 3), and what
//! [`crate::validate`] reads right now to check a parsed view. Nothing in
//! `kubuno-ui` is touched to produce it.
//!
//! ## Deviation from the design note's worked example
//!
//! §4's sketch stores the setter *closure* directly in the property table
//! entry the interpreter would later call. Phase 2a explicitly excludes the
//! interpreter (§8's phased table), and there is no interpreter-neutral way
//! to type-erase five different concrete `kubuno_ui` component types into one
//! `fn(AnyComponent, &str) -> AnyComponent` table without guessing at a
//! design the interpreter phase (2c) has not made yet (a `Box<dyn Any>`
//! scheme? a hand-written enum of the five types? something the widget tree
//! itself needs to stay consistent with?). Inventing that here risks
//! becoming the second, drifting source of truth this note explicitly warns
//! against (§4, §8's 2a risk note).
//!
//! So this registry keeps the setter *table* purely descriptive — name,
//! [`PropKind`], default, doc — the same information §4 says the schema/LSP/
//! property-grid consumers need, none of which requires calling into
//! `kubuno-ui`. The link to the real API is kept anyway, just moved from
//! "the registry calls it at runtime" to "the registry declaration calls it
//! in a generated test": [`macros::component`] takes a real constructor
//! expression and a `smoke` closure that chains the same real setters the
//! properties describe, compiled into a `#[test]` per component. A renamed
//! or removed `kubuno-ui` builder method is therefore a `cargo build`/
//! `cargo test` failure, not a silent drift — the exact mitigation §8's 2a
//! row asks for ("a small `cargo test` that at least calls every registered
//! ctor/setter once"), just wired through the type system that is actually
//! available today rather than one this phase would have had to invent.

pub(crate) mod components;
/// The JSON export of this registry — `vskubuno/docs/DESIGNER.md` §5/§8, work
/// package DSG-1. See that module's own doc for the wire shape and where it
/// deliberately follows the already-written C# consumer
/// (`vskubuno/src/Kubuno.VisualStudio.Designer/Registry/ComponentRegistry.
/// FromJson`) instead of §5's own sketch where the two disagree.
pub mod docs_fr;
pub mod export;
pub mod families;
mod macros;
/// The registry open to applications (EVT-7b): linked and declared classes.
pub mod project;
pub use project::{
    declared, project_components, project_info, register, register_class, set_declared, ClassKind, ClassRegistration, DeclaredComponent, DeclaredEvent, DeclaredKind,
    DeclaredProperty, Origin, ProjectInfo, Registered,
};
#[doc(hidden)]
pub use project::__private;
// The metadata types (`PropKind`, `PropertyMeta`, `EventMeta`, `LevelMeta`, `ChildrenModel`,
// `LayoutKind`, the `d:` design-time attributes) live in the platform-neutral `kubuno-views-model`
// (`vskubuno/docs/WEB-VIEWS.md` WV-1) and are re-exported here under their historical paths; the
// tables below and `ComponentMeta` stay in this crate.
pub use kubuno_views_model::meta::{
    design_time_attribute, ChildrenModel, DesignTimeAttribute, EventCategory, EventMeta, LayoutKind, LevelMeta, PropKind, PropertyMeta, Routing,
    DESIGN_TIME_ATTRIBUTES,
};

/// The events every control raises (`vskubuno/docs/EVENTS.md` §3's catalogue, the part
/// the input router of EVT-2 raises for any element): mouse, keyboard, focus, validation,
/// size and location. A component's own table overrides an entry of the same name (a
/// `<Button>`'s own `OnClick` doc). Not offered on the structural child elements
/// (`<Item>`, `<Column>`, `<TabItem>`…, see [`is_gated`]), which are not controls.
pub const COMMON_EVENTS: &[EventMeta] = {
    use crate::events::{CancelEventArgs, KeyEventArgs, KeyPressEventArgs, MouseEventArgs};
    use EventCategory::{Focus, Key, Layout, Mouse, PropertyChanged};
    &[
        EventMeta::new("OnClick", "Occurs when the control is clicked.").args::<MouseEventArgs>(),
        EventMeta::new("OnDoubleClick", "Occurs when the control is double-clicked.").args::<MouseEventArgs>(),
        EventMeta::new("OnMouseClick", "Occurs when the control is clicked with the mouse.").args::<MouseEventArgs>(),
        EventMeta::new("OnMouseDoubleClick", "Occurs when the control is double-clicked with the mouse.").args::<MouseEventArgs>(),
        EventMeta::new("OnMouseDown", "Occurs when a mouse button is pressed over the control.").category(Mouse).args::<MouseEventArgs>(),
        EventMeta::new("OnMouseUp", "Occurs when a mouse button pressed over the control is released.").category(Mouse).args::<MouseEventArgs>(),
        EventMeta::new("OnMouseMove", "Occurs when the mouse pointer moves over the control.").category(Mouse).args::<MouseEventArgs>(),
        EventMeta::new("OnMouseEnter", "Occurs when the mouse pointer enters the control.").category(Mouse),
        EventMeta::new("OnMouseLeave", "Occurs when the mouse pointer leaves the control.").category(Mouse),
        EventMeta::new("OnMouseHover", "Occurs when the mouse pointer rests on the control.").category(Mouse),
        EventMeta::new("OnMouseWheel", "Occurs when the mouse wheel moves while the pointer is over the control.")
            .category(Mouse)
            .args::<MouseEventArgs>(),
        EventMeta::new("OnKeyDown", "Occurs when a key is pressed while the control has the focus.").category(Key).args::<KeyEventArgs>(),
        EventMeta::new("OnKeyPress", "Occurs when a character key is pressed while the control has the focus.")
            .category(Key)
            .args::<KeyPressEventArgs>(),
        EventMeta::new("OnKeyUp", "Occurs when a key is released while the control has the focus.").category(Key).args::<KeyEventArgs>(),
        EventMeta::new("OnEnter", "Occurs when the control, or a control inside it, receives the focus.").category(Focus),
        EventMeta::new("OnGotFocus", "Occurs when the control receives the focus.").category(Focus),
        EventMeta::new("OnLeave", "Occurs when the focus leaves the control and every control inside it.").category(Focus),
        EventMeta::new("OnLostFocus", "Occurs when the control loses the focus.").category(Focus),
        EventMeta::new("OnValidating", "Occurs when the control is validating, before the focus leaves it. Cancel it to keep the focus.")
            .category(Focus)
            .args::<CancelEventArgs>(),
        EventMeta::new("OnValidated", "Occurs when the control has finished validating.").category(Focus),
        EventMeta::new("OnResize", "Occurs when the control is resized.").category(Layout),
        EventMeta::new("OnMove", "Occurs when the control is moved.").category(Layout),
        EventMeta::new("OnSizeChanged", "Occurs when the size of the control changes.").category(PropertyChanged),
        EventMeta::new("OnLocationChanged", "Occurs when the position of the control changes.").category(PropertyChanged),
        EventMeta::new("OnDragDrop", "Occurs when data (files, text…) is dropped on the control. Its AllowDrop property must be true, and DragEnter or DragOver must have accepted it (e.effect).")
            .category(EventCategory::DragDrop)
            .args::<crate::events::DragEventArgs>(),
        EventMeta::new("OnDragEnter", "Occurs when data is dragged over the control. Set e.effect to accept the drop. Its AllowDrop property must be true.")
            .category(EventCategory::DragDrop)
            .args::<crate::events::DragEventArgs>(),
        EventMeta::new("OnDragOver", "Occurs while data is dragged over the control. Set e.effect to accept the drop at that point.")
            .category(EventCategory::DragDrop)
            .args::<crate::events::DragEventArgs>(),
        EventMeta::new("OnDragLeave", "Occurs when data dragged over the control leaves it or the drag is cancelled.").category(EventCategory::DragDrop),
        EventMeta::new("OnPaint", "Occurs when the control is drawn: draw on e.graphics(). Raised by custom controls, PaintBox elements and buttons.")
            .category(EventCategory::Appearance)
            .args::<crate::events::PaintEventArgs>(),
    ]
};

/// The events of the view itself, accepted on its ROOT element only (`vskubuno/docs/
/// EVENTS.md` §3, "View (root)"): Load → Activated → Shown when the view first appears,
/// Activated/Deactivate as its window gains or loses the focus, FormClosing (cancelable) →
/// FormClosed when its window closes.
pub const VIEW_EVENTS: &[EventMeta] = &[
    EventMeta::new("OnLoad", "Occurs before the view is shown for the first time.").category(EventCategory::Behavior),
    EventMeta::new("OnShown", "Occurs the first time the view is shown.").category(EventCategory::Behavior),
    EventMeta::new("OnActivated", "Occurs when the window of the view becomes active.").category(EventCategory::Focus),
    EventMeta::new("OnDeactivate", "Occurs when the window of the view stops being active.").category(EventCategory::Focus),
    EventMeta::new("OnFormClosing", "Occurs before the window of the view closes. Cancel it to keep the window open (unsaved changes).")
        .category(EventCategory::Behavior)
        .args::<crate::events::FormClosingEventArgs>(),
    EventMeta::new("OnFormClosed", "Occurs after the window of the view has closed, just before it is destroyed.")
        .category(EventCategory::Behavior)
        .args::<crate::events::FormClosedEventArgs>(),
    EventMeta::new("OnResizeBegin", "Occurs when the user starts moving or resizing the window.").category(EventCategory::Layout),
    EventMeta::new("OnResizeEnd", "Occurs when the user has finished moving or resizing the window.").category(EventCategory::Layout),
    EventMeta::new("OnTitleBarDoubleClick", "Occurs when the title bar is double-clicked (the window then maximizes or restores).").category(EventCategory::Action),
    EventMeta::new("OnDpiChanged", "Occurs when the window moves to a display of another scale.")
        .category(EventCategory::Layout)
        .args::<crate::events::DpiChangedEventArgs>(),
    EventMeta::new("OnHelpButtonClicked", "Occurs when the help button of the title bar is clicked.").category(EventCategory::Behavior),
    EventMeta::new("OnCaptionButtonClick", "Occurs when one of your title bar buttons (CaptionButtons) is clicked: e.id says which.")
        .category(EventCategory::Action)
        .args::<crate::events::CaptionButtonEventArgs>(),
    EventMeta::new("OnMdiChildActivate", "Occurs when an MDI document becomes active in the window, or closes.").category(EventCategory::Behavior),
    // The header's standard items (ShowSearch, ShowNotifications, ShowSettings, ShowHelp).
    EventMeta::new("OnSearchClicked", "Occurs when the search button of the title bar (ShowSearch) is clicked.").category(EventCategory::Action),
    EventMeta::new("OnNotificationsClicked", "Occurs when the notifications bell of the title bar (ShowNotifications) is clicked.").category(EventCategory::Action),
    EventMeta::new("OnSettingsClicked", "Occurs when the settings button of the title bar (ShowSettings) is clicked.").category(EventCategory::Action),
    EventMeta::new("OnHelpClicked", "Occurs when the header's help button of the title bar (ShowHelp) is clicked.").category(EventCategory::Action),
];

/// The properties each level declares (see that module) and the view's own.
pub mod common;
pub use common::{view_property, VIEW_PROPERTIES};

/// The levels of the control hierarchy, root first. `Control` declares the common events
/// ([`COMMON_EVENTS`]) and properties ([`common::CONTROL_PROPERTIES`]); the view events
/// ([`VIEW_EVENTS`]) and properties ([`VIEW_PROPERTIES`]) are `View`'s, accepted on a view's root
/// element whatever its class.
pub const LEVELS: &[LevelMeta] = &[
    LevelMeta { name: "Component", doc: "A component: sited in a view, disposable; the non-visual base.", events: &[], default_event: None, properties: &[] },
    LevelMeta {
        name: "Control",
        doc: "A visual component: bounds, focus, mouse and keyboard.",
        events: COMMON_EVENTS,
        default_event: Some("OnClick"),
        properties: common::CONTROL_PROPERTIES,
    },
    LevelMeta {
        name: "ScrollableControl",
        doc: "A control that scrolls its content.",
        events: &[],
        default_event: None,
        properties: common::SCROLLABLE_PROPERTIES,
    },
    LevelMeta { name: "ContainerControl", doc: "A control that manages the focus of the controls inside it.", events: &[], default_event: None, properties: &[] },
    LevelMeta { name: "UserControl", doc: "A composite control designed as a view of its own.", events: &[], default_event: Some("OnLoad"), properties: &[] },
    LevelMeta { name: "View", doc: "A view in its own window (the form).", events: VIEW_EVENTS, default_event: Some("OnLoad"), properties: VIEW_PROPERTIES },
    LevelMeta { name: "ButtonBase", doc: "The buttons.", events: &[], default_event: Some("OnClick"), properties: common::BUTTON_BASE_PROPERTIES },
    LevelMeta { name: "TextBoxBase", doc: "The text fields.", events: &[], default_event: Some("OnTextChanged"), properties: common::TEXT_BOX_BASE_PROPERTIES },
    LevelMeta { name: "ListControl", doc: "The lists.", events: &[], default_event: Some("OnSelectionChanged"), properties: common::LIST_CONTROL_PROPERTIES },
    LevelMeta { name: "LabelBase", doc: "The text displays.", events: &[], default_event: None, properties: common::LABEL_BASE_PROPERTIES },
    LevelMeta { name: "ContainerBase", doc: "The layout containers.", events: &[], default_event: None, properties: common::CONTAINER_BASE_PROPERTIES },
    LevelMeta { name: "RangeBase", doc: "The value-in-a-range controls.", events: &[], default_event: Some("OnValueChanged"), properties: common::RANGE_BASE_PROPERTIES },
    LevelMeta { name: "RibbonControl", doc: "The elements of a ribbon (tabs, groups, commands): the ribbon lays them out.", events: &[], default_event: Some("OnClick"), properties: families::ribbon::RIBBON_CONTROL_PROPERTIES },
    LevelMeta { name: "RibbonItem", doc: "What stands in a ribbon group, its quick access toolbar or a menu.", events: &[], default_event: Some("OnClick"), properties: families::ribbon::RIBBON_ITEM_PROPERTIES },
];

/// The level named `name`.
pub fn level(name: &str) -> Option<&'static LevelMeta> {
    LEVELS.iter().find(|l| l.name == name)
}

/// The view event (root element only) whose attribute is `name` (or an alias of it).
pub fn view_event(name: &str) -> Option<&'static EventMeta> {
    VIEW_EVENTS.iter().find(|e| e.matches(name))
}

/// The common event (see [`COMMON_EVENTS`]) named `name` (or an alias of it).
pub fn common_event(name: &str) -> Option<&'static EventMeta> {
    COMMON_EVENTS.iter().find(|e| e.matches(name))
}

/// Whether `name` is a structural child element of the default registry (`<Item>`,
/// `<Column>`, `<TabItem>`…): one that some component's [`ChildrenModel::List`] gates,
/// not a control. Such an element only has the events its own table declares.
pub fn is_gated(name: &str) -> bool {
    all().iter().any(|c| matches!(c.children, ChildrenModel::List(allowed) if allowed.contains(&name)))
}

/// Whether `name` is a non-visual component (EVT-7b): an element whose class is not a `Control`
/// and that no parent gates — a `<Timer>`, an application's `#[kubuno(extends = Component)]` class.
/// It paints nothing; the designer lists it in the component tray under the design surface.
pub fn is_non_visual(name: &str) -> bool {
    lookup(name).is_some_and(|m| !m.base_chain().is_empty() && !m.is_a("Control")) && !is_gated(name)
}

/// The signature of [`ComponentMeta::build`] — named so the field itself
/// stays legible (clippy's `type_complexity` lint, correctly, does not want
/// the raw `fn(...) -> Result<...>` spelled out twice: here and at every call
/// site that names it).
pub type BuildFn = fn(&crate::props::Props<'_>, &mut crate::props::BuildCx) -> Result<Box<dyn crate::node::ViewNode>, crate::props::BuildError>;

/// One entry of the registry: everything known about one XML element.
#[derive(Clone, Copy)]
pub struct ComponentMeta {
    /// The element name, exactly as it appears in a `.kbview` file — the
    /// real `kubuno_ui` builder's own name (§1: "one source of truth").
    pub name: &'static str,
    pub doc: &'static str,
    pub properties: &'static [PropertyMeta],
    pub events: &'static [EventMeta],
    pub children: ChildrenModel,
    /// See [`LayoutKind`]. Defaults to [`LayoutKind::None`] when a
    /// `component!` declaration omits `layout:` (the `component!` macro's own
    /// doc explains why that is the common case).
    pub layout: LayoutKind,
    /// `true` for a "free-form data record" element whose real fields are
    /// dynamically named by ITS PARENT rather than fixed on the component
    /// itself — `<Item>`'s own doc is the worked example: a
    /// `<ListView>`/`<DataTable>` row's per-column value is read straight off
    /// an `<Item>` attribute named exactly like that column's own `Binding`
    /// field (`registry::families::data::static_item_rows`), which no closed
    /// `properties` list could enumerate (a `.kbview` file's own `<Column>`
    /// children decide the names). Defaults to `false` when a `component!`
    /// declaration omits `open_attributes:` — [`crate::validate::validate`]'s
    /// "unknown attribute" check is skipped entirely for an element with this
    /// set, same as it already skips the closed-enum/type check for a
    /// `{Binding …}` expression (§3: resolved at runtime, not statically
    /// checkable) — an open-attributes element's *shape* is not statically
    /// checkable either, for the same reason.
    pub open_attributes: bool,
    /// The event a double-click on the element in the designer creates a handler for
    /// (WinForms' `[DefaultEvent]`, `vskubuno/docs/EVENTS.md` §3), when the `component!`
    /// declaration names one; see [`ComponentMeta::default_event`] for the fallback.
    pub default_event: Option<&'static str>,
    /// Phase 2c (the interpreter, `vskubuno/docs/XML_VIEWS.md` §8): builds
    /// the real `kubuno_ui` widget this element describes, reading every
    /// property through [`crate::props::Props`]'s typed accessors — the
    /// *same* table as `properties` above, not a second hand-written setter
    /// list (see `crate::props`' module doc). Generated by the `component!`
    /// macro's `build:` clause alongside `properties`/`events`/`children`,
    /// so one call site still declares everything about a component.
    pub build: BuildFn,
}

// `ComponentMeta` no longer derives `Debug`: a `fn` item field prints as an
// address, which is not useful, and `#[derive(Debug)]` would print it anyway
// (function pointers ARE `Debug`) — this note exists only because dropping
// the derive is a visible change from phase 2a for anyone diffing this file.
// It was never depended on: nothing in this crate or its tests formats a
// `ComponentMeta` with `{:?}`.

impl ComponentMeta {
    /// The property whose attribute is `name` (or an older alias of it): the component's own, else
    /// the one a level of its chain declares (`Enabled`, `BackColor`… for every control). View
    /// properties ([`VIEW_PROPERTIES`]) are not included: they belong to the root element, which only
    /// the caller knows.
    pub fn property(&self, name: &str) -> Option<&'static PropertyMeta> {
        self.own_property(name).or_else(|| self.inherited_property(name).map(|(_, p)| p))
            .or_else(|| self.icon_option(name))
    }

    /// The component's own property named `name` (or an alias of it).
    pub fn own_property(&self, name: &str) -> Option<&'static PropertyMeta> {
        self.properties.iter().find(|p| p.matches(name))
    }

    /// A property the element inherits from a level of its chain, with the level that declares it.
    pub fn inherited_property(&self, name: &str) -> Option<(&'static str, &'static PropertyMeta)> {
        self.levels().find_map(|level| level.properties.iter().find(|p| p.matches(name)).map(|p| (level.name, p)))
    }

    /// Every property of the element: its own first, then those its levels declare (nearest level
    /// first) that it does not redeclare, each with the level it comes from (`None`: its own).
    pub fn all_properties(&self) -> Vec<(Option<&'static str>, &'static PropertyMeta)> {
        let mut out: Vec<(Option<&'static str>, &'static PropertyMeta)> = self.properties.iter().map(|p| (None, p)).collect();
        for level in self.levels() {
            for p in level.properties {
                if !out.iter().any(|(_, o)| o.name == p.name) {
                    out.push((Some(level.name), p));
                }
            }
        }
        if self.has_icon() {
            for p in common::ICON_OPTION_PROPERTIES {
                if !out.iter().any(|(_, o)| o.name == p.name) {
                    out.push((None, p));
                }
            }
        }
        out
    }

    /// Whether the element has an icon property (`editor("icon")`): it then also takes
    /// [`common::ICON_OPTION_PROPERTIES`] (`IconSize`, `IconScaling`, `IconColor`).
    pub fn has_icon(&self) -> bool {
        self.properties.iter().any(PropertyMeta::is_icon) || self.levels().any(|l| l.properties.iter().any(PropertyMeta::is_icon))
    }

    /// The icon option `name` when the element has an icon.
    fn icon_option(&self, name: &str) -> Option<&'static PropertyMeta> {
        if !self.has_icon() {
            return None;
        }
        common::ICON_OPTION_PROPERTIES.iter().find(|p| p.matches(name))
    }

    /// The canonical property of an attribute that is an older alias (`Max` → `Maximum`).
    pub fn property_alias_target(&self, name: &str) -> Option<&'static PropertyMeta> {
        self.property(name).filter(|p| p.name != name)
    }

    /// The event whose attribute is `name` — the component's own (by name or alias) or,
    /// for a control, a [`COMMON_EVENTS`] one. View events ([`VIEW_EVENTS`]) are not
    /// included: they belong to the root element, which only the caller knows.
    pub fn event(&self, name: &str) -> Option<&'static EventMeta> {
        self.events.iter().find(|e| e.matches(name)).or_else(|| self.inherited_event(name).map(|(_, e)| e))
    }

    /// An event the element inherits from a level of its chain (EVT-7a: the `Control` level
    /// declares [`COMMON_EVENTS`]), with the level that declares it.
    pub fn inherited_event(&self, name: &str) -> Option<(&'static str, &'static EventMeta)> {
        self.levels().find_map(|level| level.events.iter().find(|e| e.matches(name)).map(|e| (level.name, e)))
    }

    /// The element's class followed by its ancestors, `"Component"` last (EVT-7a):
    /// `["Button", "ButtonBase", "Control", "Component"]` — what the export, the Toolbox and the
    /// `is`-checks read. Empty for an element without a class.
    pub fn base_chain(&self) -> &'static [&'static str] {
        crate::controls::class_of(self.name).map(|c| c.chain).unwrap_or(&[])
    }

    /// Whether the element's class is `name` or derives from it (`is_a("ButtonBase")`).
    pub fn is_a(&self, name: &str) -> bool {
        self.base_chain().contains(&name)
    }

    /// The levels of the element's chain, nearest first (its own class excluded).
    pub fn levels(&self) -> impl Iterator<Item = &'static LevelMeta> {
        self.base_chain().iter().skip(1).filter_map(|n| level(n))
    }

    /// Whether the element raises [`COMMON_EVENTS`]: its class is a `Control` (every control;
    /// not the structural child elements, which are non-visual components — see [`is_gated`]).
    pub fn has_common_events(&self) -> bool {
        match crate::controls::class_of(self.name) {
            Some(_) => self.is_a("Control"),
            None => !is_gated(self.name),
        }
    }

    /// Every event of the element: its own table first, then the events its levels declare
    /// (the common events) that it does not override (same attribute name).
    pub fn all_events(&self) -> Vec<&'static EventMeta> {
        let mut out: Vec<&'static EventMeta> = self.events.iter().collect();
        for level in self.levels() {
            for e in level.events {
                if !out.iter().any(|o| o.name == e.name) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// The default event (designer double-click): the declared one, else the nearest level's
    /// default the element has (`Control`'s is `OnClick`), else its first own event; `None` for
    /// an element with no event.
    pub fn default_event(&self) -> Option<&'static str> {
        if let Some(name) = self.default_event {
            return self.event(name).map(|e| e.name);
        }
        self.levels()
            .filter_map(|l| l.default_event)
            .find_map(|name| self.event(name).map(|e| e.name))
            .or_else(|| self.events.first().map(|e| e.name))
    }

    /// The canonical event of an attribute that is an older alias (`OnToggled` on a
    /// `<Switch>` → its `OnCheckedChanged`), or `None` when `name` is not an alias.
    pub fn alias_target(&self, name: &str) -> Option<&'static EventMeta> {
        self.event(name).filter(|e| e.name != name)
    }
}

/// Every declared component, in declaration order. Kept as a flat slice
/// (rather than a `HashMap`, built once with `OnceLock`) — five to a few
/// dozen entries never justifies the extra machinery; [`lookup`] is a linear
/// scan and is fast enough for a file that is parsed once per edit, not once
/// per frame (§5's parse+compile / bind+paint split).
pub fn all() -> &'static [ComponentMeta] {
    project::snapshot().all
}

/// The built-in elements only (the families), without the application's classes.
pub fn builtins() -> &'static [ComponentMeta] {
    // The five phase-2a examples, then every enabled family, concatenated
    // once: `const` slices of different families cannot be joined at compile
    // time, and the registry is read far more often than it is built.
    static ALL: std::sync::OnceLock<Vec<ComponentMeta>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<ComponentMeta> = components::ALL.to_vec();
        for family in families::ALL_FAMILIES {
            v.extend_from_slice(family);
        }
        v
    })
}

/// Finds a component by its XML element name (case-sensitive — element names
/// are Rust type names, and `Button` vs `button` is exactly the kind of typo
/// the validator (`crate::validate`) exists to catch).
///
/// Also answers the designer's placeholder element (`crate::tolerant::PLACEHOLDER_ELEMENT`), which is
/// in no listing ([`all`], the export, the Toolbox): only a text the designer rewrote names it, and the
/// builders that look their children up by name must find it there.
pub fn lookup(element_name: &str) -> Option<&'static ComponentMeta> {
    all()
        .iter()
        .find(|c| c.name == element_name)
        .or_else(|| crate::tolerant::placeholder_meta(element_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_component_is_findable_by_name() {
        for c in builtins() {
            assert_eq!(lookup(c.name).map(|m| m.name), Some(c.name));
        }
    }

    #[test]
    fn registry_is_not_empty() {
        // The five phase-2a examples are always present; each enabled family
        // only adds to them.
        assert!(builtins().len() >= components::ALL.len());
        assert!(components::ALL.len() >= 3);
    }

    #[test]
    fn element_names_are_unique_across_families() {
        let mut seen = std::collections::HashSet::new();
        for c in builtins() {
            assert!(seen.insert(c.name), "`{}` is declared twice", c.name);
        }
    }

    #[test]
    fn unknown_name_is_none() {
        assert!(lookup("NotAComponent").is_none());
    }

    #[test]
    fn every_default_event_exists_on_its_component() {
        for c in builtins() {
            if let Some(name) = c.default_event {
                assert!(c.event(name).is_some(), "{}'s default event {name} is not one of its events", c.name);
                assert_eq!(c.default_event(), Some(name), "{}", c.name);
            }
            if c.has_common_events() {
                let default = c.default_event().unwrap_or_else(|| panic!("{} has no default event", c.name));
                assert!(c.event(default).is_some(), "{}", c.name);
            }
        }
    }

    #[test]
    fn default_events_follow_the_design_note() {
        let default = |n: &str| lookup(n).and_then(|c| c.default_event());
        assert_eq!(default("Button"), Some("OnClick"));
        assert_eq!(default("Switch"), Some("OnCheckedChanged"));
        assert_eq!(default("TextField"), Some("OnTextChanged"));
        assert_eq!(default("Stack"), Some("OnClick"));
        assert_eq!(default("Card"), Some("OnClick"));
    }

    #[test]
    fn aliases_resolve_to_the_canonical_event() {
        let switch = lookup("Switch").unwrap();
        assert_eq!(switch.event("OnToggled").map(|e| e.name), Some("OnCheckedChanged"));
        assert_eq!(switch.alias_target("OnToggled").map(|e| e.name), Some("OnCheckedChanged"));
        assert!(switch.alias_target("OnCheckedChanged").is_none());
        let text = lookup("TextField").unwrap();
        assert_eq!(text.event("OnChanged").map(|e| e.name), Some("OnTextChanged"));
    }

    #[test]
    fn common_events_are_on_controls_and_own_events_override_them() {
        let button = lookup("Button").unwrap();
        assert_eq!(button.event("OnMouseDown").map(|e| e.category), Some(EventCategory::Mouse));
        let clicks: Vec<_> = button.all_events().into_iter().filter(|e| e.name == "OnClick").collect();
        assert_eq!(clicks.len(), 1);
        assert!(clicks[0].doc.contains("Space"), "the button's own OnClick doc wins");
        assert!(button.event("OnValidating").is_some_and(|e| e.cancelable && e.args_type == "CancelEventArgs"));
        assert!(button.event("OnLoad").is_none(), "view events are root-only");
        assert!(view_event("OnLoad").is_some());
    }

    /// EVT-7a: the chain decides what an element inherits, and it agrees with the rules the
    /// registry followed before it (common events on every non-structural element).
    #[test]
    fn metadata_flows_from_the_base_chain() {
        for c in builtins() {
            assert!(!c.base_chain().is_empty(), "{} has a class", c.name);
            // A ribbon element is gated (only valid in its parent) and still a control (RIBBON.md §3).
            let control = (!is_gated(c.name) || c.is_a("RibbonControl")) && !is_non_visual(c.name);
            assert_eq!(c.has_common_events(), control, "{}", c.name);
            assert_eq!(c.is_a("Control"), control, "{}", c.name);
            for level in c.base_chain().iter().skip(1) {
                // A class may derive from another class (`RibbonToggleButton` from `RibbonButton`).
                assert!(level_meta_exists(level) || crate::controls::class_of(level).is_some(), "{}: unknown level {level}", c.name);
            }
        }
        let button = lookup("Button").unwrap();
        assert_eq!(button.base_chain(), ["Button", "ButtonBase", "Control", "Component"]);
        assert!(button.is_a("ButtonBase") && !button.is_a("ListControl"));
        assert_eq!(button.inherited_event("OnMouseDown").map(|(l, e)| (l, e.name)), Some(("Control", "OnMouseDown")));
        assert!(button.inherited_event("OnClick").is_some() && button.events.iter().any(|e| e.name == "OnClick"), "Button overrides Click's doc");
        assert_eq!(lookup("TabItem").unwrap().base_chain(), ["TabItem", "Component"]);
        assert!(lookup("TabItem").unwrap().inherited_event("OnMouseDown").is_none());
        // A ProgressBar (a RangeBase without ValueChanged) falls back to Control's OnClick.
        assert_eq!(lookup("ProgressBar").unwrap().default_event(), Some("OnClick"));
    }

    fn level_meta_exists(name: &str) -> bool {
        level(name).is_some()
    }

    #[test]
    fn event_names_and_aliases_are_unique_per_component() {
        for c in builtins() {
            let mut seen = std::collections::HashSet::new();
            for e in c.all_events() {
                assert!(e.name.starts_with("On"), "{}.{}", c.name, e.name);
                assert!(seen.insert(e.name), "{}.{} declared twice", c.name, e.name);
                for a in e.aliases {
                    assert!(seen.insert(a), "{}.{} alias clashes", c.name, a);
                }
            }
        }
    }
}
