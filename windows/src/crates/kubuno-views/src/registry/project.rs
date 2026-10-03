//! The registry open to applications (EVT-7b of `vskubuno/docs/EVENTS.md`, §4.3 "The registry
//! becomes extensible"): the built-in families, then every class an application compiles in with
//! `#[derive(Component)]` / `#[derive(UserControl)]` (**linked** classes: registered by a static
//! constructor the derive emits, before `main`), then the classes the tools only know from the
//! source (**declared** classes: the language server's `syn` scan of the workspace, the design
//! surface's list from Visual Studio before the project is built — they validate, complete and
//! show in the Properties window, and render as a labelled placeholder).
//!
//! The element name is the XML name space (the note's rule): a class named like a built-in
//! element is refused (the built-in wins, an error is logged), two linked classes of the same name
//! keep the first one (an error names both crates), and a declared class never shadows a linked
//! one. [`crate::registry::all`] and [`crate::registry::lookup`] see everything; the merged
//! metadata of a class inherits its base's properties, events, children and layout, so
//! `<RoundButton Text="Ok" Variant="Secondary" CornerRadius="18"/>` validates like a `<Button>`
//! plus its own properties.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Mutex, PoisonError, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::{ChildrenModel, ComponentMeta, EventCategory, EventMeta, LayoutKind, PropKind, PropertyMeta};
use crate::component::Component;
use crate::controls::ClassRef;

/// What a registered class is, for the tools: a control, a user control (a composite `.kbview`)
/// or a non-visual component (WinForms' component tray).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    Control,
    UserControl,
    Component,
}

impl ClassKind {
    /// The exported name (`"control"`, `"user_control"`, `"component"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            ClassKind::Control => "control",
            ClassKind::UserControl => "user_control",
            ClassKind::Component => "component",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "user_control" => ClassKind::UserControl,
            "component" => ClassKind::Component,
            _ => ClassKind::Control,
        }
    }
}

/// What `#[derive(Component)]` / `#[derive(UserControl)]` compiles for a class of an application
/// (a `static`, registered by [`register_class`]).
pub struct ClassRegistration {
    /// The class and element name.
    pub name: &'static str,
    /// The crate that declares it (`CARGO_CRATE_NAME`).
    pub crate_name: &'static str,
    pub kind: ClassKind,
    /// The description (`#[description]` or the doc comment).
    pub doc: &'static str,
    /// The base named by `extends` (a built-in class, a level, or another class).
    pub extends: &'static str,
    /// The class followed by its ancestors, `"Component"` last.
    pub chain: &'static [&'static str],
    /// A fresh instance (`None` when the class has no `Default`: it cannot be created from a view).
    pub create: fn() -> Option<Rc<RefCell<dyn Component>>>,
    /// The class's own properties (its `#[property]` fields); the base's are inherited.
    pub properties: &'static [PropertyMeta],
    /// The class's own events (its `#[event]` fields).
    pub events: &'static [EventMeta],
    pub default_event: Option<&'static str>,
    pub default_property: Option<&'static str>,
    pub toolbox_category: Option<&'static str>,
    pub toolbox_icon: Option<&'static str>,
    /// Offered in the Toolbox.
    pub browsable: bool,
    /// A user control's `.kbview` text, embedded at compile time.
    pub view: Option<&'static str>,
    /// Its path, relative to the file declaring the class.
    pub view_path: Option<&'static str>,
    /// The absolute folder of its view when it was compiled: what the relative paths of the view
    /// (`d:ItemsSource="design/rows.json"`, image files) resolve against when another view — maybe in
    /// another folder of the project — nests it.
    pub view_dir: Option<&'static str>,
    /// The file declaring the class (`file!()`).
    pub source_file: &'static str,
}

/// Implemented by `#[derive(Component)]`: the class's registration, for [`register`].
pub trait Registered {
    fn registration() -> &'static ClassRegistration;
}

/// A class the tools know from its source only (see the module doc). Deserializes from the
/// language server's `kubuno/registry` export entries (`vskubuno` forwards the project's entries
/// to the design surface with them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclaredComponent {
    pub name: String,
    /// `"control"`, `"user_control"` or `"component"`.
    pub kind: String,
    pub doc: String,
    pub crate_name: Option<String>,
    pub extends: String,
    /// The class followed by its ancestors, `"Component"` last.
    pub base_chain: Vec<String>,
    pub properties: Vec<DeclaredProperty>,
    pub events: Vec<DeclaredEvent>,
    pub default_event: Option<String>,
    pub default_property: Option<String>,
    pub toolbox_category: Option<String>,
    pub toolbox_icon: Option<String>,
    pub browsable: bool,
    pub view_path: Option<String>,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
}

impl Default for DeclaredComponent {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: "control".to_string(),
            doc: String::new(),
            crate_name: None,
            extends: String::new(),
            base_chain: Vec::new(),
            properties: Vec::new(),
            events: Vec::new(),
            default_event: None,
            default_property: None,
            toolbox_category: None,
            toolbox_icon: None,
            browsable: true,
            view_path: None,
            source_file: None,
            source_line: None,
        }
    }
}

/// The kind of a declared property, in the export's shape (`"Bool"`, `"F32"`, `"String"`,
/// `{"Enum": [...]}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclaredKind {
    Bool,
    F32,
    #[default]
    String,
    Enum(Vec<String>),
}

/// One property of a [`DeclaredComponent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclaredProperty {
    pub name: String,
    pub kind: DeclaredKind,
    pub default: String,
    pub doc: String,
    pub category: Option<String>,
    pub browsable: bool,
    pub bindable: bool,
    pub localizable: bool,
    pub serialization: Option<String>,
    pub editor: Option<String>,
    pub type_converter: Option<String>,
}

impl Default for DeclaredProperty {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: DeclaredKind::String,
            default: String::new(),
            doc: String::new(),
            category: None,
            browsable: true,
            bindable: false,
            localizable: false,
            serialization: None,
            editor: None,
            type_converter: None,
        }
    }
}

/// One event of a [`DeclaredComponent`]. Inherited and view events of an export entry
/// (`inherited_from`, `root_only`) are skipped when it is registered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclaredEvent {
    pub name: String,
    pub doc: String,
    pub category: String,
    /// The args type (`"MouseEventArgs"`, or a type of the project).
    pub args_type: String,
    pub browsable: bool,
    pub inherited_from: Option<String>,
    pub root_only: bool,
}

impl Default for DeclaredEvent {
    fn default() -> Self {
        Self { name: String::new(), doc: String::new(), category: "Action".to_string(), args_type: "EventArgs".to_string(), browsable: true, inherited_from: None, root_only: false }
    }
}

/// Where a project class comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Compiled into this program (its derive registered it).
    Linked,
    /// Known from its source only (a placeholder).
    Declared,
}

/// What the registry knows of a project class beyond its [`ComponentMeta`].
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub name: &'static str,
    pub origin: Origin,
    pub kind: ClassKind,
    pub crate_name: Option<&'static str>,
    pub extends: &'static str,
    pub toolbox_category: Option<&'static str>,
    pub toolbox_icon: Option<&'static str>,
    pub browsable: bool,
    pub default_property: Option<&'static str>,
    /// The class's own properties (set on its instance from the element's attributes).
    pub own_properties: &'static [PropertyMeta],
    /// A linked user control's `.kbview` text.
    pub view: Option<&'static str>,
    pub view_path: Option<&'static str>,
    /// The absolute folder of a linked user control's view (see [`ClassRegistration::view_dir`]).
    pub view_dir: Option<&'static str>,
    pub source_file: Option<&'static str>,
    pub source_line: Option<u32>,
    /// Its node does not raise Click itself (a user control, a control drawn from a level): the input router
    /// raises it, as it does for a `<Panel>`.
    pub routed_click: bool,
}

/// The registry's current contents (rebuilt when a class is registered or the declared set
/// changes; readers keep the `'static` snapshot they were given).
pub(crate) struct Snapshot {
    pub(crate) all: &'static [ComponentMeta],
    pub(crate) classes: &'static [ClassRef],
    pub(crate) infos: &'static [ProjectInfo],
    /// The generation of the registrations it was built from.
    pub(crate) gen: u64,
}

static PENDING: Mutex<Vec<&'static ClassRegistration>> = Mutex::new(Vec::new());
static DECLARED: Mutex<Vec<DeclaredComponent>> = Mutex::new(Vec::new());
/// Bumped by every change of the registrations (see [`snapshot`]); snapshots record the one they
/// were built from. Starts above any snapshot's (none is built yet).
static GENERATION: AtomicU64 = AtomicU64::new(1);
static CURRENT: RwLock<Option<&'static Snapshot>> = RwLock::new(None);
static REBUILD: Mutex<()> = Mutex::new(());

/// Registers a class compiled into the program. Called by the static constructor
/// `#[derive(Component)]` emits (before `main`: it only records the class), or by hand through
/// [`register`]. Registering the same class twice is harmless.
pub fn register_class(registration: &'static ClassRegistration) {
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    if !pending.iter().any(|r| std::ptr::eq(*r, registration)) {
        pending.push(registration);
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Registers the class `T` by hand (where static constructors do not run).
pub fn register<T: Registered>() {
    register_class(T::registration());
}

/// Replaces the declared classes (the language server after a workspace scan, the design surface
/// when Visual Studio sends the project's classes). A linked class of the same name wins.
pub fn set_declared(components: Vec<DeclaredComponent>) {
    let mut declared = DECLARED.lock().unwrap_or_else(PoisonError::into_inner);
    if *declared != components {
        *declared = components;
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// The declared classes last set.
pub fn declared() -> Vec<DeclaredComponent> {
    DECLARED.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub(crate) fn snapshot() -> &'static Snapshot {
    // A reader never gets a snapshot older than the last change it can see: every change bumps the
    // generation AFTER recording itself, and a snapshot is built from the registrations read after
    // its generation was read. (A dirty flag cleared before the rebuild used to hand a concurrent
    // reader the previous snapshot, without a class it had just registered.)
    let current = |target: u64| (*CURRENT.read().unwrap_or_else(PoisonError::into_inner)).filter(|s| s.gen >= target);
    if let Some(s) = current(GENERATION.load(Ordering::Acquire)) {
        return s;
    }
    let _guard = REBUILD.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        let target = GENERATION.load(Ordering::Acquire);
        if let Some(s) = current(target) {
            return s;
        }
        let mut built = build();
        built.gen = target;
        let built: &'static Snapshot = Box::leak(Box::new(built));
        *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Some(built);
    }
}

/// Every project class (linked, then declared), with what the registry knows of it.
pub fn project_components() -> &'static [ProjectInfo] {
    snapshot().infos
}

/// The project class named `name`.
pub fn project_info(name: &str) -> Option<&'static ProjectInfo> {
    snapshot().infos.iter().find(|i| i.name == name)
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_slice<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

/// The registry meta of the nearest ancestor of `chain` that is an element (a built-in class or a
/// project class already merged), skipping `chain[0]` (the class itself).
fn base_meta<'a>(chain: &[&str], metas: &'a [ComponentMeta]) -> Option<&'a ComponentMeta> {
    chain.iter().skip(1).find_map(|name| metas.iter().find(|m| m.name == *name))
}

/// A class's merged meta: its own properties and events, then the base's that it does not
/// redeclare; the base's children model and layout (none for a user control or a component).
#[allow(clippy::too_many_arguments)]
fn merged(
    name: &'static str,
    doc: &'static str,
    kind: ClassKind,
    own_props: &'static [PropertyMeta],
    own_events: &'static [EventMeta],
    default_event: Option<&'static str>,
    base: Option<&ComponentMeta>,
    base_is_project: bool,
    build: super::BuildFn,
) -> ComponentMeta {
    let mut properties: Vec<PropertyMeta> = own_props.to_vec();
    let mut events: Vec<EventMeta> = own_events.to_vec();
    let (children, layout) = match (kind, base) {
        (ClassKind::Control, Some(b)) => (b.children, b.layout),
        _ => (ChildrenModel::None, LayoutKind::None),
    };
    if let Some(b) = base {
        // A user control does not take the `<UserControl>` root element's properties, but an inherited user control
        // takes those of the user control it extends (a project class: its properties are declared ones).
        if kind != ClassKind::UserControl || base_is_project {
            properties.extend(b.properties.iter().filter(|p| !own_props.iter().any(|o| o.name == p.name)).copied());
        }
        events.extend(b.events.iter().filter(|e| !own_events.iter().any(|o| o.name == e.name)).copied());
    }
    ComponentMeta {
        name,
        doc,
        properties: leak_slice(properties),
        events: leak_slice(events),
        children,
        layout,
        open_attributes: false,
        default_event: default_event.or_else(|| base.and_then(|b| b.default_event)),
        build,
    }
}

fn build() -> Snapshot {
    let mut all: Vec<ComponentMeta> = super::builtins().to_vec();
    let builtin_count = all.len();
    let mut classes: Vec<ClassRef> = Vec::new();
    let mut infos: Vec<ProjectInfo> = Vec::new();

    // Linked classes, bases before the classes extending them (a base's chain is shorter).
    let mut linked: Vec<&'static ClassRegistration> = PENDING.lock().unwrap_or_else(PoisonError::into_inner).clone();
    linked.sort_by_key(|r| r.chain.len());
    for reg in linked {
        if all[..builtin_count].iter().any(|m| m.name == reg.name) {
            tracing::error!(class = reg.name, krate = reg.crate_name, "a control class is named like a built-in element: the built-in `<{}>` is used", reg.name);
            continue;
        }
        if let Some(other) = infos.iter().find(|i| i.name == reg.name) {
            tracing::error!(
                class = reg.name,
                first = other.crate_name.unwrap_or("?"),
                second = reg.crate_name,
                "two control classes named `{}` (crates `{}` and `{}`): the first one is used",
                reg.name,
                other.crate_name.unwrap_or("?"),
                reg.crate_name
            );
            continue;
        }
        let base = base_meta(reg.chain, &all).copied();
        // A user control and a control drawn from a level have no node raising Click (a built-in class's does, a
        // `<Button>`'s): the input router raises it on a press and release over them.
        let (build, routed_click): (super::BuildFn, bool) = match reg.kind {
            ClassKind::UserControl => (crate::node::custom::user_control_build, true),
            ClassKind::Component => (crate::node::custom::non_visual_build, false),
            ClassKind::Control => match base {
                Some(b) => (b.build, false),
                None => (crate::node::custom::custom_control_build, true),
            },
        };
        let base_is_project = base.is_some_and(|b| infos.iter().any(|i| i.name == b.name));
        all.push(merged(reg.name, reg.doc, reg.kind, reg.properties, reg.events, reg.default_event, base.as_ref(), base_is_project, build));
        classes.push(ClassRef { name: reg.name, chain: reg.chain, create: reg.create });
        infos.push(ProjectInfo {
            name: reg.name,
            origin: Origin::Linked,
            kind: reg.kind,
            crate_name: Some(reg.crate_name),
            extends: reg.extends,
            toolbox_category: reg.toolbox_category,
            toolbox_icon: reg.toolbox_icon,
            browsable: reg.browsable,
            default_property: reg.default_property,
            own_properties: reg.properties,
            view: reg.view,
            view_path: reg.view_path,
            view_dir: reg.view_dir,
            source_file: Some(reg.source_file),
            source_line: None,
            routed_click,
        });
    }

    // Declared classes the program does not link, bases first.
    let mut declared = DECLARED.lock().unwrap_or_else(PoisonError::into_inner).clone();
    declared.sort_by_key(|d| d.base_chain.len());
    for d in declared {
        if d.name.is_empty() || all.iter().any(|m| m.name == d.name) {
            continue;
        }
        let name = leak(&d.name);
        let chain: Vec<&'static str> = if d.base_chain.first().is_some_and(|n| n == &d.name) {
            d.base_chain.iter().map(|s| leak(s)).collect()
        } else {
            std::iter::once(name).chain(d.base_chain.iter().map(|s| leak(s))).collect()
        };
        let chain = leak_slice(chain);
        let kind = ClassKind::parse(&d.kind);
        let own_props = leak_slice(d.properties.iter().map(declared_property).collect());
        let own_events = leak_slice(d.events.iter().filter(|e| e.inherited_from.is_none() && !e.root_only).map(declared_event).collect());
        let base = base_meta(chain, &all).copied();
        let build: super::BuildFn = if kind == ClassKind::Component { crate::node::custom::non_visual_build } else { crate::node::custom::placeholder_build };
        let default_event = d.default_event.as_deref().map(leak);
        let base_is_project = base.is_some_and(|b| infos.iter().any(|i| i.name == b.name));
        all.push(merged(name, leak(&d.doc), kind, own_props, own_events, default_event, base.as_ref(), base_is_project, build));
        classes.push(ClassRef { name, chain, create: no_instance });
        infos.push(ProjectInfo {
            name,
            origin: Origin::Declared,
            kind,
            crate_name: d.crate_name.as_deref().map(leak),
            extends: leak(&d.extends),
            toolbox_category: d.toolbox_category.as_deref().map(leak),
            toolbox_icon: d.toolbox_icon.as_deref().map(leak),
            browsable: d.browsable,
            default_property: d.default_property.as_deref().map(leak),
            own_properties: own_props,
            view: None,
            view_path: d.view_path.as_deref().map(leak),
            view_dir: None,
            source_file: d.source_file.as_deref().map(leak),
            source_line: d.source_line,
            routed_click: false,
        });
    }

    Snapshot { all: leak_slice(all), classes: leak_slice(classes), infos: leak_slice(infos), gen: 0 }
}

fn no_instance() -> Option<Rc<RefCell<dyn Component>>> {
    None
}

fn declared_property(p: &DeclaredProperty) -> PropertyMeta {
    let kind = match &p.kind {
        DeclaredKind::Bool => PropKind::Bool,
        DeclaredKind::F32 => PropKind::F32,
        DeclaredKind::String => PropKind::String,
        DeclaredKind::Enum(variants) => PropKind::Enum(leak_slice(variants.iter().map(|v| leak(v)).collect())),
    };
    let mut meta = PropertyMeta::new(leak(&p.name), kind, leak(&p.default), leak(&p.doc));
    meta.category = p.category.as_deref().map(leak);
    meta.browsable = p.browsable;
    meta.bindable = p.bindable;
    meta.localizable = p.localizable;
    meta.serialization = p.serialization.as_deref().map(leak);
    meta.editor = p.editor.as_deref().map(leak);
    meta.type_converter = p.type_converter.as_deref().map(leak);
    meta
}

/// The standard args types, for a declared event naming one (`"MouseEventArgs"`): their chain,
/// Rust type and writability, exactly as `EventMeta::args::<A>()` reads them.
const STANDARD_ARGS: &[EventMeta] = {
    use crate::events::*;
    &[
        EventMeta::new("", "").args::<EmptyEventArgs>(),
        EventMeta::new("", "").args::<HandledEventArgs>(),
        EventMeta::new("", "").args::<MouseEventArgs>(),
        EventMeta::new("", "").args::<KeyEventArgs>(),
        EventMeta::new("", "").args::<KeyPressEventArgs>(),
        EventMeta::new("", "").args::<PaintEventArgs>(),
        EventMeta::new("", "").args::<CancelEventArgs>(),
        EventMeta::new("", "").args::<FormClosingEventArgs>(),
        EventMeta::new("", "").args::<FormClosedEventArgs>(),
        EventMeta::new("", "").args::<DragEventArgs>(),
        EventMeta::new("", "").args::<ScrollEventArgs>(),
        EventMeta::new("", "").args::<LayoutEventArgs>(),
        EventMeta::new("", "").args::<TextChangedEventArgs>(),
        EventMeta::new("", "").args::<CheckedChangedEventArgs>(),
        EventMeta::new("", "").args::<SelectionChangedEventArgs>(),
        EventMeta::new("", "").args::<NumericValueChangedEventArgs>(),
        EventMeta::new("", "").args::<ItemEventArgs>(),
        EventMeta::new("", "").args::<ItemActivateEventArgs>(),
        EventMeta::new("", "").args::<ItemCheckEventArgs>(),
        EventMeta::new("", "").args::<CellEventArgs>(),
        EventMeta::new("", "").args::<CellCancelEventArgs>(),
        EventMeta::new("", "").args::<CellValidatingEventArgs>(),
        EventMeta::new("", "").args::<PropertyChangedEventArgs>(),
        EventMeta::new("", "").args::<HotReloadedEventArgs>(),
    ]
};

/// The category named `name` (the export's English names).
pub fn event_category(name: &str) -> EventCategory {
    use EventCategory::*;
    [Action, Behavior, Data, DragDrop, Focus, Key, Layout, Mouse, PropertyChanged, Appearance].into_iter().find(|c| c.name() == name).unwrap_or(Behavior)
}

fn declared_event(e: &DeclaredEvent) -> EventMeta {
    let mut meta = EventMeta::new(leak(&e.name), leak(&e.doc)).category(event_category(&e.category));
    let args = e.args_type.as_str();
    if let Some(std) = STANDARD_ARGS.iter().find(|s| s.args_rust == args || s.args_type == args) {
        meta.args_type = std.args_type;
        meta.args_chain = std.args_chain;
        meta.args_rust = std.args_rust;
        meta.args_mut = std.args_mut;
        meta.cancelable = std.cancelable;
    } else if !args.is_empty() && args != "EventArgs" {
        let name = leak(args);
        meta.args_type = name;
        meta.args_chain = leak_slice(vec![name, "EventArgs"]);
        meta.args_rust = name;
    }
    meta.browsable = e.browsable;
    meta
}

/// Plumbing of the derives (autoref specialization over traits the class may or may not
/// implement). Not public API.
#[doc(hidden)]
pub mod __private {
    use std::cell::RefCell;
    use std::marker::PhantomData;
    use std::rc::Rc;

    use crate::binding::ViewModel;
    use crate::component::Component;
    use crate::events::{dispatch_typed, ElementRef, EventArgs, EventSink};

    /// `(&Probe::<T>::new()).method()` picks the specialized trait when `T` qualifies.
    pub struct Probe<T>(PhantomData<T>);

    impl<T> Probe<T> {
        #[allow(clippy::new_without_default)]
        pub const fn new() -> Self {
            Probe(PhantomData)
        }
    }

    /// A class with `Default` can be created from a view.
    pub trait CreateDefault {
        fn kubuno_create(&self) -> Option<Rc<RefCell<dyn Component>>>;
    }

    impl<T: Component + Default> CreateDefault for Probe<T> {
        fn kubuno_create(&self) -> Option<Rc<RefCell<dyn Component>>> {
            Some(Rc::new(RefCell::new(T::default())))
        }
    }

    /// … one without cannot.
    pub trait CreateNone {
        fn kubuno_create(&self) -> Option<Rc<RefCell<dyn Component>>> {
            None
        }
    }

    impl<T> CreateNone for &Probe<T> {}

    /// A user control with an `#[event_handlers]` impl runs its handlers.
    pub trait SinkDispatch<T> {
        fn kubuno_dispatch(&self, vm: &mut T, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool;
    }

    impl<T: ViewModel + EventSink> SinkDispatch<T> for Probe<T> {
        fn kubuno_dispatch(&self, vm: &mut T, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
            dispatch_typed(vm, handler, sender, args)
        }
    }

    /// … one without has none.
    pub trait SinkNone<T> {
        fn kubuno_dispatch(&self, _vm: &mut T, _handler: &str, _sender: &ElementRef<'_>, _args: &mut dyn EventArgs) -> bool {
            false
        }
    }

    impl<T> SinkNone<T> for &Probe<T> {}
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
