//! The registry metadata types (`vskubuno/docs/XML_VIEWS.md` §4, `docs/VIEWS-SPEC.md` §10): what one
//! element, property, event, hierarchy level and design-time attribute *is*, independent of any
//! renderer. Moved out of `kubuno-desktop-views`' `registry` module by WV-1 (`vskubuno/docs/WEB-VIEWS.md`),
//! which still re-exports every item here under its old path (`kubuno_desktop_views::registry::PropKind`…);
//! the tables themselves (the built-in families, the common events and levels) and
//! `ComponentMeta` — whose `build` function and class chain are the desktop runtime's — stay there.


/// The Rust type family a property's XML attribute is validated against.
/// Deliberately small — exactly what `XML_VIEWS.md` §4 lists ("Rust type …
/// `enum` variants when the type is a closed enum") and what the five
/// example components in `kubuno_desktop_views::registry::components` actually use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Bool,
    /// A plain DIP number (`Width`, `Gap`, a uniform `Padding`…). Several of
    /// the design note's worked-example attributes (`Padding="XL"`,
    /// `Gap="none"`) suggest a named-token sugar over these; no such closed
    /// enum exists in `kubuno-desktop-ui`/`kubuno-desktop-controls` today (`Padding` is a
    /// plain `{f32 × 4}` box, a flow gap is a plain `f32` field) — inventing
    /// one here would be exactly the "restate, don't own" mistake
    /// `kubuno-desktop-ui/src/lib.rs`'s rule 1 warns against. Resolving a token name
    /// to a DIP number is left to the interpreter phase that actually reads
    /// `kubuno_desktop_controls`'s spacing scale.
    F32,
    String,
    /// A closed set of attribute values, in declaration order — the source
    /// for a completion dropdown and for the validator's "not a valid value
    /// for this enum" diagnostic.
    Enum(&'static [&'static str]),
}

/// One property: a declared XML attribute the element accepts.
#[derive(Debug, Clone, Copy)]
pub struct PropertyMeta {
    pub name: &'static str,
    pub kind: PropKind,
    /// The value used when the attribute is absent, as literal text (what a
    /// property-grid or a hover tooltip shows) — not parsed back into a
    /// typed value here; nothing in phase 2a needs to construct a live
    /// default, only to display one.
    pub default: &'static str,
    /// Lifted from the real builder's `///` doc comment where practical, kept
    /// as a literal here because a `macro_rules!` cannot read a doc comment
    /// off another crate's item (§4).
    pub doc: &'static str,
    /// The Properties window group (WinForms `[Category]`), when the declaration names one (a
    /// custom control's `#[category("Appearance")]`, EVT-7b); the designer's own table groups
    /// the built-in properties otherwise.
    pub category: Option<&'static str>,
    /// Listed in the Properties window (`#[browsable(false)]` hides it; still valid in XML).
    pub browsable: bool,
    /// Meant to be bound (`#[property(bindable)]`, WinForms `[Bindable(true)]`).
    pub bindable: bool,
    /// `#[localizable]`.
    pub localizable: bool,
    /// `#[designer_serialization_visibility(…)]`: `"Visible"`, `"Hidden"` or `"Content"`.
    pub serialization: Option<&'static str>,
    /// `#[editor("…")]`: the Properties window editor to use (`"color"`…).
    pub editor: Option<&'static str>,
    /// `#[type_converter("…")]`: how the Properties window reads and writes the text (`"Padding"`,
    /// `"Size"`, `"Color"`, `"Font"`, `"Opacity"`…).
    pub type_converter: Option<&'static str>,
    /// Older attribute names that still mean this property (`Max` for `Maximum`), never removed
    /// within 1.x — read like the canonical name.
    pub aliases: &'static [&'static str],
    /// Read by the designer only (`Locked`, `Modifiers`…); the running application ignores it.
    pub design_time: bool,
}

impl PropertyMeta {
    pub const fn new(name: &'static str, kind: PropKind, default: &'static str, doc: &'static str) -> Self {
        Self {
            name,
            kind,
            default,
            doc,
            category: None,
            browsable: true,
            bindable: false,
            localizable: false,
            serialization: None,
            editor: None,
            type_converter: None,
            aliases: &[],
            design_time: false,
        }
    }

    /// Older attribute names still accepted for this property.
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Read by the designer only.
    pub const fn design_time(mut self) -> Self {
        self.design_time = true;
        self
    }

    /// Whether `attribute` names this property: its own name or one of its aliases.
    pub fn matches(&self, attribute: &str) -> bool {
        self.name == attribute || self.aliases.contains(&attribute)
    }

    /// The Properties window group.
    pub const fn category(mut self, category: &'static str) -> Self {
        self.category = Some(category);
        self
    }

    /// Not listed in the Properties window (still valid in XML).
    pub const fn hidden(mut self) -> Self {
        self.browsable = false;
        self
    }

    /// Meant to be bound.
    pub const fn bindable(mut self) -> Self {
        self.bindable = true;
        self
    }

    /// Localizable.
    pub const fn localizable(mut self) -> Self {
        self.localizable = true;
        self
    }

    /// The designer serialization visibility (`"Visible"`, `"Hidden"`, `"Content"`).
    pub const fn serialization(mut self, visibility: &'static str) -> Self {
        self.serialization = Some(visibility);
        self
    }

    /// The Properties window editor.
    pub const fn editor(mut self, editor: &'static str) -> Self {
        self.editor = Some(editor);
        self
    }

    /// The type converter.
    pub const fn type_converter(mut self, converter: &'static str) -> Self {
        self.type_converter = Some(converter);
        self
    }

    /// The editor a property's value type asks for (`kubuno_desktop_views::component::PropertyValue::EDITOR`:
    /// `"list"` for `Rows`, `"object"` for `Shared<T>` — set with a binding only, so also
    /// bindable —, `"color"` for a colour, `"lines"` for a `Vec<String>`), unless one is already set.
    pub const fn value_editor(mut self, editor: Option<&'static str>) -> Self {
        if let (Some(e), None) = (editor, self.editor) {
            self.editor = Some(e);
            if const_str_eq(e, "list") || const_str_eq(e, "object") {
                self.bindable = true;
            }
        }
        self
    }

    /// An icon (`editor("icon")`): a name of the Kubuno icon set, an image file or a resource —
    /// completed, checked and picked as such (`kubuno_desktop_views::icon`).
    pub fn is_icon(&self) -> bool {
        matches!(self.editor, Some("icon"))
    }

    /// Set with a binding only (a list or an object, see [`Self::value_editor`]).
    pub fn is_bound_only(&self) -> bool {
        matches!(self.editor, Some("list") | Some("object"))
    }
}

/// The Properties window group an event is listed under (`vskubuno/docs/EVENTS.md`
/// §5.1: the ⚡ tab is grouped by category, like WinForms' `[Category]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventCategory {
    Action,
    Behavior,
    Data,
    DragDrop,
    Focus,
    Key,
    Layout,
    Mouse,
    PropertyChanged,
    Appearance,
}

impl EventCategory {
    /// The English category name, as exported (`"Drag Drop"`, `"Property Changed"`…).
    pub const fn name(self) -> &'static str {
        match self {
            EventCategory::Action => "Action",
            EventCategory::Behavior => "Behavior",
            EventCategory::Data => "Data",
            EventCategory::DragDrop => "Drag Drop",
            EventCategory::Focus => "Focus",
            EventCategory::Key => "Key",
            EventCategory::Layout => "Layout",
            EventCategory::Mouse => "Mouse",
            EventCategory::PropertyChanged => "Property Changed",
            EventCategory::Appearance => "Appearance",
        }
    }
}

/// How an event travels (`vskubuno/docs/EVENTS.md` §3): raised directly on the element
/// concerned (every event today, like WinForms), or bubbling to an ancestor when unhandled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Routing {
    Direct,
    Bubble,
}

/// One event an element can raise — the XML side of `OnClick="handler_name"` (§2), plus
/// the metadata of `vskubuno/docs/EVENTS.md` §5.1 (EVT-3): the ⚡ tab's category, the args
/// type and its ancestor chain (handler compatibility), cancelable, routing, and the older
/// attribute names still accepted for it (`OnToggled` for `OnCheckedChanged` on a
/// `<Switch>`). Built with [`EventMeta::new`] and the `const` builder methods below, so a
/// family table stays one line per event.
#[derive(Debug, Clone, Copy)]
pub struct EventMeta {
    /// The attribute name, `On` + the event's display name (`"OnClick"`).
    pub name: &'static str,
    pub doc: &'static str,
    pub category: EventCategory,
    /// The args type's tooling name ([`ArgsChain::NAME`]), `"EventArgs"` by default.
    pub args_type: &'static str,
    /// The args type followed by its ancestors, root last ([`ArgsChain::CHAIN`]).
    pub args_chain: &'static [&'static str],
    /// The Rust type a typed handler declares for the args ([`ArgsChain::RUST_TYPE`]).
    pub args_rust: &'static str,
    /// A handler writes back into the args ([`ArgsChain::WRITABLE`]).
    pub args_mut: bool,
    /// A handler can cancel it (its args are a `CancelEventArgs`).
    pub cancelable: bool,
    pub routing: Routing,
    /// Older attribute names that still mean this event (never removed within 1.x).
    pub aliases: &'static [&'static str],
    /// Listed in the Properties window's ⚡ tab.
    pub browsable: bool,
}

impl EventMeta {
    /// An `Action` event with the root `EventArgs`, direct, no alias, browsable.
    pub const fn new(name: &'static str, doc: &'static str) -> Self {
        Self {
            name,
            doc,
            category: EventCategory::Action,
            args_type: "EventArgs",
            args_chain: &["EventArgs"],
            args_rust: "EmptyEventArgs",
            args_mut: false,
            cancelable: false,
            routing: Routing::Direct,
            aliases: &[],
            browsable: true,
        }
    }

    pub const fn category(mut self, category: EventCategory) -> Self {
        self.category = category;
        self
    }

    /// The args type `A` (its name and chain); cancelable when `A` is a `CancelEventArgs`.
    pub const fn args<A: ArgsChain>(mut self) -> Self {
        self.args_type = A::NAME;
        self.args_chain = A::CHAIN;
        self.args_rust = A::RUST_TYPE;
        self.args_mut = A::WRITABLE;
        let mut i = 0;
        while i < A::CHAIN.len() {
            if const_str_eq(A::CHAIN[i], "CancelEventArgs") {
                self.cancelable = true;
            }
            i += 1;
        }
        self
    }

    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    pub const fn routing(mut self, routing: Routing) -> Self {
        self.routing = routing;
        self
    }

    /// Not listed in the ⚡ tab (still valid in XML).
    pub const fn hidden(mut self) -> Self {
        self.browsable = false;
        self
    }

    /// The name shown in the ⚡ tab and the docs: the attribute without its `On` prefix.
    pub fn display_name(&self) -> &'static str {
        self.name.strip_prefix("On").unwrap_or(self.name)
    }

    /// Whether `attribute` names this event: its own name or one of its aliases.
    pub fn matches(&self, attribute: &str) -> bool {
        self.name == attribute || self.aliases.contains(&attribute)
    }
}

const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The compile-time name and ancestor chain of an args type, generated by
/// `#[derive(EventArgs)]` (`kubuno_desktop_views::events`). `#[args(extends = P)]` requires `P: ArgsChain`.
/// Declared here, beside [`EventMeta::args`] which reads it, and re-exported as
/// `kubuno_desktop_views::events::ArgsChain` (where its usage example lives).
///
/// ```
/// use kubuno_desktop_views_model::ArgsChain;
///
/// struct SaveEventArgs;
/// impl ArgsChain for SaveEventArgs {
///     const NAME: &'static str = "SaveEventArgs";
///     const CHAIN: &'static [&'static str] = &["SaveEventArgs", "CancelEventArgs", "EventArgs"];
/// }
///
/// let meta = kubuno_desktop_views_model::EventMeta::new("OnSave", "Saving.").args::<SaveEventArgs>();
/// assert_eq!(meta.args_type, "SaveEventArgs");
/// assert!(meta.cancelable);
/// assert_eq!(meta.args_rust, "SaveEventArgs");
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an event args type",
    label = "not an `EventArgs`",
    note = "a handler's args are a standard type such as `MouseEventArgs` or `KeyEventArgs`, `&dyn EventArgs` for any event, or your own struct with `#[derive(EventArgs)]`"
)]
pub trait ArgsChain {
    /// The type's own display name (`"MouseEventArgs"`).
    const NAME: &'static str;
    /// [`Self::NAME`] followed by every ancestor's name, root (`"EventArgs"`) last.
    const CHAIN: &'static [&'static str];
    /// The Rust type a handler declares for these args (`"MouseEventArgs"`, `"EmptyEventArgs"`
    /// for the root, `"TextChangedEventArgs"` for `ValueChangedEventArgs<String>`): what
    /// `kubuno/createHandler` writes in a typed stub. [`Self::NAME`] by default.
    const RUST_TYPE: &'static str = Self::NAME;
    /// A handler writes back into these args (`handled`, `cancel`): its stub takes them as
    /// `&mut`. Set by the derive's `#[args(handled)]` / `#[args(cancel)]`.
    const WRITABLE: bool = false;
}

/// One level of the control hierarchy (EVT-7a, `kubuno_desktop_views::component`) as the registry sees it:
/// the events it declares — which every element whose class derives from it inherits — and its
/// default event (the designer double-click's, for an element that declares none).
#[derive(Debug, Clone, Copy)]
pub struct LevelMeta {
    pub name: &'static str,
    pub doc: &'static str,
    pub events: &'static [EventMeta],
    pub default_event: Option<&'static str>,
    /// The properties the level declares, which every element whose class derives from it inherits
    /// (`vskubuno/docs/EVENTS.md` §16).
    pub properties: &'static [PropertyMeta],
}

/// How an element accepts children — §4's "the children model
/// (`none` / `single_widget` / `list<Item>`)".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildrenModel {
    /// No children allowed (`<Switch/>`, `<TextField/>`…).
    None,
    /// At most one child element (`<Card>`'s body).
    SingleWidget,
    /// Any number of child elements, in document order (`<Stack>`'s blocks).
    ///
    /// The slice names the "gated" child element names this component
    /// explicitly hosts — the structural, parent-only elements such as
    /// `<TabItem>`/`<Item>`/`<Column>`/`<Option>`/`<Step>`/
    /// `<AccordionSection>`/`<BreadcrumbItem>`/`<ToolbarItem>`, each valid
    /// only directly inside the handful of parents that list it. Empty
    /// (`&[]`) for an ordinary widget container (`<Stack>`, `<Panel>`,
    /// `<Splitter>`…) that takes any component as a child and gates nothing.
    ///
    /// `kubuno_desktop_views::validate::validate` is the sole authority on what this
    /// gates: a child element name that appears in *some* component's
    /// `allowed` list anywhere in the registry is "gated" everywhere, and
    /// only valid directly under a parent whose own `allowed` names it —
    /// this is what lets `<TabItem>` be rejected under `<Stack>` (whose own
    /// `allowed` is empty) exactly as it would be rejected under `<Accordion>`
    /// (whose `allowed` is `&["AccordionSection"]`, not `&["TabItem"]`),
    /// with no separate "required parent" field to keep in sync by hand —
    /// see that module's `gated_required_parents` helper.
    List(&'static [&'static str]),
}

/// A descriptive tag for which of `kubuno_desktop_controls::layout_panels`' engines a
/// container drives — `vskubuno/docs/DESIGNER.md` §4/§6 (DSG-1): "the
/// designer needs a `LayoutKind` (or equivalent) alongside `ChildrenModel`
/// … before its drop visualization [can] pick the right cue". Metadata only
/// (this phase does not build a designer): it does not change how
/// `kubuno_desktop_views::registry::ComponentMeta::build` constructs anything, it only *describes* what the
/// four engines named in `XML_VIEWS.md` §1 already do, for a future consumer
/// (the designer's drag/drop, the language server's hover) to read instead of
/// re-deriving from the component's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// A leaf, or a `SingleWidget`/gated-`List` body with no layout engine of
    /// its own to describe (most components — the default `layout:` value
    /// the `component!` macro fills in when a declaration omits the field).
    None,
    /// `<Stack>`'s flow engine (`kubuno_desktop_controls::layout_panels::flow_layout`):
    /// children placed one after another along `Direction`, no `X`/`Y`.
    Flow,
    /// `<Panel>`'s Dock/Anchor engine: `Dock`-ed children stack in document
    /// order (§1: "document order IS z-order"); `Anchor`-ed children get free
    /// `X`/`Y` plus edge-stretch.
    DockAnchor,
    /// `<Splitter>`'s two-pane split (`kubuno_desktop_controls::layout_panels::
    /// SplitContainer`): exactly two children, `Distance` is the divider.
    Split,
    /// `<Tabs>`'s paged layout: one `<TabItem>` visible at a time — not a
    /// spatial arrangement of its children at all, which is why it is its
    /// own kind rather than reusing `List`'s generic shape.
    Tabs,
}

/// A design-time attribute: accepted on the view's ROOT element only, read by the visual designer
/// and ignored at runtime (nothing builds or lays out from it) — the `.kbview` counterpart of
/// XAML's `d:DesignWidth`/`d:DesignHeight`.
#[derive(Debug, Clone, Copy)]
pub struct DesignTimeAttribute {
    pub name: &'static str,
    pub kind: PropKind,
    pub doc: &'static str,
    pub doc_fr: &'static str,
}

/// The designer's canvas size for a view whose root element has no `Width`/`Height` of its own
/// (a view normally fills whatever window shows it, so it usually has none). The designer writes
/// these when the view is resized on its canvas.
pub const DESIGN_TIME_ATTRIBUTES: &[DesignTimeAttribute] = &[
    DesignTimeAttribute {
        name: "DesignWidth",
        kind: PropKind::F32,
        doc: "Width of the view in the designer, in pixels (design time only, ignored when the app runs).",
        doc_fr: "Largeur de la vue dans le concepteur, en pixels (conception uniquement, ignorée à l'exécution).",
    },
    DesignTimeAttribute {
        name: "DesignHeight",
        kind: PropKind::F32,
        doc: "Height of the view in the designer, in pixels (design time only, ignored when the app runs).",
        doc_fr: "Hauteur de la vue dans le concepteur, en pixels (conception uniquement, ignorée à l'exécution).",
    },
];

/// The design-time attribute named `name`, if it is one.
pub fn design_time_attribute(name: &str) -> Option<&'static DesignTimeAttribute> {
    DESIGN_TIME_ATTRIBUTES.iter().find(|a| a.name == name)
}
