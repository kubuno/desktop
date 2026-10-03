//! # `kubuno-desktop-views-meta` — the design-time grammar of Kubuno controls
//!
//! Work package EVT-7b of `vskubuno/docs/EVENTS.md` (§4.3, "How the tools learn about user
//! components"): a project's own controls — `#[derive(Component)]` classes and
//! `#[derive(UserControl)]` composites — are described by attributes on their struct, WinForms'
//! `System.ComponentModel` attributes in Rust:
//!
//! ```text
//! /// A button drawn as a pill.                     (the description, like [Description])
//! #[derive(Component, Default)]
//! #[kubuno(extends = Button, overrides(Control))]  (the base class, EVT-7a)
//! #[category("Kubuno")]                            (the Toolbox group)
//! #[toolbox(icon = "circle")]                      (the Toolbox icon)
//! #[default_event("Click")]  #[default_property("CornerRadius")]
//! pub struct RoundButton {
//!     base: Button,
//!     #[property]
//!     #[category("Appearance")]
//!     #[description("The radius of the corners, in pixels.")]
//!     #[default_value(18.0)]
//!     pub corner_radius: f32,
//!     #[event]
//!     #[category("Action")]
//!     #[description("Occurs when the button is pressed for a long time.")]
//!     pub long_press: Event<MouseEventArgs>,
//! }
//! ```
//!
//! Two readers must agree exactly on what that means: the proc macros of `kubuno-desktop-views-macros`
//! (which compile it into the control's registration) and the language server `kubuno-desktop-views-ls`
//! (which scans the workspace's `.rs` files with `syn`, so a control is known to the `.kbview`
//! editor, the Properties window and the Toolbox as soon as it is typed, before any build). Both
//! call [`parse_decl`]; the language server finds the structs with [`scan_source`]. Nothing here
//! knows `kubuno-desktop-views` itself: the result is a plain description ([`ComponentDecl`]).
//!
//! The level/class tables of the control hierarchy (EVT-7a) live here too ([`LEVELS`],
//! [`CLASSES`]): the macros need them to know the levels of a built-in base class they cannot
//! see, the language server to compute a scanned control's base chain.

/// A dependency-free reader of `.kbview` files (the `#[kubuno_desktop::view]` macro reads the view it
/// generates the members of).
pub mod kbview;

/// Visual inheritance of views (`x:Inherits`): a view merged over its base view.
pub mod inherit;

/// The framework's own crates, an explicit list the tools skip when they look for an application's control
/// libraries (never a `kubuno` prefix: third-party crates are named `kubuno-…` too).
pub mod framework;
pub use framework::{is_framework_crate, is_library_component_crate, FRAMEWORK_CRATES, LIBRARY_COMPONENT_CRATES};

use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Ident, Item, Lit, LitStr, Meta, Path, Type};

// ── The control hierarchy (EVT-7a) ─────────────────────────────────────────────────────────

/// The levels of the hierarchy: name, parent level, snake-case stem of its methods
/// (`control_core`, `base_control`, `as_control`).
pub const LEVELS: &[(&str, Option<&str>, &str)] = &[
    ("Component", None, "component"),
    ("Control", Some("Component"), "control"),
    ("ScrollableControl", Some("Control"), "scrollable_control"),
    ("ContainerControl", Some("ScrollableControl"), "container_control"),
    ("UserControl", Some("ContainerControl"), "user_control"),
    ("View", Some("ContainerControl"), "view"),
    ("ButtonBase", Some("Control"), "button_base"),
    ("TextBoxBase", Some("Control"), "text_box_base"),
    ("ListControl", Some("Control"), "list_control"),
    ("LabelBase", Some("Control"), "label_base"),
    ("ContainerBase", Some("ScrollableControl"), "container_base"),
    ("RangeBase", Some("Control"), "range_base"),
    ("RibbonControl", Some("Control"), "ribbon_control"),
    ("RibbonItem", Some("RibbonControl"), "ribbon_item"),
];

/// The built-in classes of `kubuno_desktop_views::controls` and the level each derives from — what lets
/// `#[kubuno(extends = Button)]` know the levels of a class the macro cannot see. `kubuno-desktop-views`
/// checks this table against its real classes in a test.
pub const CLASSES: &[(&str, &str)] = &[
    ("Button", "ButtonBase"),
    ("IconButton", "ButtonBase"),
    ("CheckBox", "ButtonBase"),
    ("RadioButton", "ButtonBase"),
    ("Switch", "ButtonBase"),
    ("TextField", "TextBoxBase"),
    ("TextArea", "TextBoxBase"),
    ("MaskedField", "TextBoxBase"),
    ("SearchField", "TextBoxBase"),
    ("ListBox", "ListControl"),
    ("CheckedListBox", "ListControl"),
    ("ComboBox", "ListControl"),
    ("Dropdown", "ListControl"),
    ("Label", "LabelBase"),
    ("LinkLabel", "LabelBase"),
    ("Badge", "LabelBase"),
    ("Slider", "RangeBase"),
    ("ProgressBar", "RangeBase"),
    ("NumericField", "RangeBase"),
    ("ScrollArea", "ScrollableControl"),
    ("Panel", "ContainerBase"),
    ("GroupBox", "ContainerBase"),
    ("FloatingWindow", "ContainerBase"),
    ("Card", "ContainerBase"),
    ("Stack", "ContainerBase"),
    ("Tabs", "ContainerBase"),
    ("Splitter", "ContainerBase"),
    ("Accordion", "ContainerBase"),
    ("DockArea", "ContainerBase"),
    ("WorkspaceShell", "ContainerBase"),
    ("Popover", "ContainerBase"),
    ("TableLayoutPanel", "ContainerBase"),
    ("Icon", "Control"),
    ("Separator", "Control"),
    ("Spinner", "Control"),
    ("Callout", "Control"),
    ("EmptyState", "Control"),
    ("Toolbar", "Control"),
    ("Breadcrumb", "Control"),
    ("Stepper", "Control"),
    ("ListView", "Control"),
    ("TreeView", "Control"),
    ("DataTable", "Control"),
    ("MonthCalendar", "Control"),
    ("DatePicker", "Control"),
    ("ColorField", "Control"),
    ("GradientField", "Control"),
    ("Repeater", "Control"),
    ("Sidebar", "Control"),
    ("StatusBar", "Control"),
    ("Avatar", "Control"),
    ("PictureBox", "Control"),
    ("SplashArtwork", "Control"),
    ("Item", "Component"),
    ("Column", "Component"),
    ("TabItem", "Component"),
    ("Option", "Component"),
    ("Step", "Component"),
    ("AccordionSection", "Component"),
    ("BreadcrumbItem", "Component"),
    ("ToolbarItem", "Component"),
    ("DockPanel", "Component"),
    ("SidebarItem", "Component"),
    ("SidebarSection", "Component"),
    ("StatusLabel", "Component"),
    ("Timer", "Component"),
    // The ribbon family (vskubuno/docs/RIBBON.md): a class's second column may name another class.
    ("Ribbon", "Control"),
    ("RibbonTab", "RibbonControl"),
    ("RibbonContextualTabGroup", "RibbonControl"),
    ("RibbonGroup", "RibbonControl"),
    ("RibbonControlGroup", "RibbonControl"),
    ("RibbonBox", "RibbonControl"),
    ("RibbonQuickAccessToolbar", "RibbonControl"),
    ("RibbonBackstage", "RibbonControl"),
    ("BackstageTab", "RibbonControl"),
    ("BackstageButton", "RibbonControl"),
    ("BackstageSeparator", "RibbonControl"),
    ("RibbonButton", "RibbonItem"),
    ("RibbonToggleButton", "RibbonButton"),
    ("RibbonRadioButton", "RibbonButton"),
    ("RibbonMenuButton", "RibbonButton"),
    ("RibbonSplitButton", "RibbonMenuButton"),
    ("RibbonColorPicker", "RibbonSplitButton"),
    ("RibbonMenuItem", "RibbonButton"),
    ("RibbonSplitMenuItem", "RibbonMenuItem"),
    ("RibbonCheckBox", "RibbonItem"),
    ("RibbonComboBox", "RibbonItem"),
    ("RibbonTextBox", "RibbonItem"),
    ("RibbonNumericField", "RibbonItem"),
    ("RibbonGallery", "RibbonItem"),
    ("RibbonGalleryCategory", "RibbonItem"),
    ("RibbonGalleryItem", "RibbonItem"),
    ("RibbonLabel", "RibbonItem"),
    ("RibbonSeparator", "RibbonItem"),
    ("Command", "Component"),
    // The menu family (vskubuno/docs/MENUS.md).
    ("MenuBar", "Control"),
    ("DropDownButton", "ButtonBase"),
    ("SplitButton", "ButtonBase"),
    ("MenuItem", "Component"),
    ("MenuSeparator", "Component"),
    ("MenuHeader", "Component"),
    ("ContextMenu", "Component"),
];

/// The level named `name`: `(name, parent, stem)`.
pub fn level(name: &str) -> Option<&'static (&'static str, Option<&'static str>, &'static str)> {
    LEVELS.iter().find(|(n, _, _)| *n == name)
}

/// `name` and its ancestor levels, root (`"Component"`) last.
pub fn level_chain(name: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut current = level(name);
    while let Some((n, parent, _)) = current {
        out.push(*n);
        current = parent.and_then(level);
    }
    out
}

/// The level a built-in class derives from (`"Button"` → `"ButtonBase"`).
pub fn class_level(name: &str) -> Option<&'static str> {
    let mut parent = CLASSES.iter().find(|(c, _)| *c == name).map(|(_, l)| *l)?;
    // A class may derive from another class (`RibbonToggleButton` from `RibbonButton`).
    while level(parent).is_none() {
        parent = CLASSES.iter().find(|(c, _)| *c == parent).map(|(_, l)| *l)?;
    }
    Some(parent)
}

/// The chain of a built-in class or a level: `"Button"` → `["Button", "ButtonBase", "Control",
/// "Component"]`, `"Control"` → `["Control", "Component"]`; empty for an unknown name.
pub fn builtin_chain(name: &str) -> Vec<&'static str> {
    if level(name).is_some() {
        return level_chain(name);
    }
    match CLASSES.iter().find(|(c, _)| *c == name) {
        Some((class, parent)) => {
            let mut out = vec![*class];
            out.extend(builtin_chain(parent));
            out
        }
        None => Vec::new(),
    }
}

/// Every level name, comma-separated (for error messages).
pub fn all_level_names() -> String {
    LEVELS.iter().map(|(n, _, _)| *n).collect::<Vec<_>>().join(", ")
}

// ── The `#[kubuno(…)]` options (EVT-7a) ─────────────────────────────────────────────────────

/// The options of `#[kubuno(extends = …, overrides(…), levels(…))]`.
#[derive(Default)]
pub struct KubunoOptions {
    pub extends: Option<Path>,
    pub overrides: Vec<Ident>,
    pub levels: Vec<Ident>,
}

/// Reads every `#[kubuno(…)]` attribute of a struct.
pub fn parse_kubuno_options(attrs: &[Attribute]) -> syn::Result<KubunoOptions> {
    let mut opts = KubunoOptions::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("kubuno")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("extends") {
                let value = meta.value()?;
                let path: Path = if value.peek(LitStr) { value.parse::<LitStr>()?.parse()? } else { value.parse()? };
                if opts.extends.replace(path).is_some() {
                    return Err(meta.error("duplicate `extends`"));
                }
                Ok(())
            } else if meta.path.is_ident("overrides") || meta.path.is_ident("levels") {
                let is_overrides = meta.path.is_ident("overrides");
                meta.parse_nested_meta(|inner| {
                    let Some(ident) = inner.path.get_ident().cloned() else {
                        return Err(inner.error("expected a level trait name, e.g. `Control`"));
                    };
                    if level(&ident.to_string()).is_none() {
                        return Err(syn::Error::new(ident.span(), format!("`{ident}` is not a level of the hierarchy (the levels are {})", all_level_names())));
                    }
                    if is_overrides { &mut opts.overrides } else { &mut opts.levels }.push(ident);
                    Ok(())
                })
            } else {
                Err(meta.error("unknown `kubuno` option (expected `extends = Base`, `overrides(Trait, …)` or `levels(Trait, …)`)"))
            }
        })?;
    }
    Ok(opts)
}

/// The field holding the base: the one marked `#[kubuno(base)]`, else the one named `base`.
pub fn base_field(input: &DeriveInput) -> syn::Result<(Ident, Type)> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(Component)]` only supports structs with named fields"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(Component)]` only supports structs with named fields (the base is a field: `base: Button`)"));
    };
    let mut marked = Vec::new();
    for f in &fields.named {
        for attr in f.attrs.iter().filter(|a| a.path().is_ident("kubuno")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("base") {
                    marked.push(f);
                    Ok(())
                } else {
                    Err(meta.error("unknown field option (expected `#[kubuno(base)]`)"))
                }
            })?;
        }
    }
    let field = match marked.as_slice() {
        [one] => *one,
        [] => match fields.named.iter().find(|f| f.ident.as_ref().is_some_and(|i| i == "base")) {
            Some(f) => f,
            None => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "a component class embeds its base: add a field `base: <the base class or core>` (or mark the field with `#[kubuno(base)]`)",
                ))
            }
        },
        [_, second, ..] => return Err(syn::Error::new_spanned(second, "only one field can be `#[kubuno(base)]`")),
    };
    let ident = field.ident.clone().ok_or_else(|| syn::Error::new_spanned(field, "the base field needs a name"))?;
    Ok((ident, field.ty.clone()))
}

// ── The declaration model ───────────────────────────────────────────────────────────────────

/// Which derive declared the class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Derive {
    /// `#[derive(Component)]`: a control (or a non-visual component) written in Rust.
    Component,
    /// `#[derive(UserControl)]`: a composite designed as a view of its own (a `.kbcontrol`).
    UserControl,
}

/// What kind of element a declared class is, for the tools: a control drawn on the surface, a
/// user control (a composite `.kbcontrol` view), or a non-visual component (WinForms' component tray:
/// a timer, a data source) whose chain has no `Control`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Control,
    UserControl,
    Component,
}

impl DeclKind {
    /// The exported name (`"control"`, `"user_control"`, `"component"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DeclKind::Control => "control",
            DeclKind::UserControl => "user_control",
            DeclKind::Component => "component",
        }
    }
}

/// The value type of a property, as the tools see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    /// Any integer or float type: an XML number.
    Number,
    /// `String`, `&'static str`, `Option<String>`.
    String,
    /// A fieldless enum deriving `PropertyValue` (its variant names, in order).
    Enum(Vec<String>),
    /// Any other type (its tokens): its `PropertyValue` implementation decides; the tools treat
    /// it as text.
    Other(String),
}

/// One property of a declared class (a field marked `#[property]` or carrying a design-time
/// attribute).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDecl {
    /// The Rust field.
    pub field: String,
    /// The XML attribute: the field in PascalCase (`corner_radius` → `CornerRadius`) or
    /// `#[property(name = "…")]`.
    pub name: String,
    /// The field's type, as written (`f32`, `Option<String>`).
    pub ty: String,
    pub kind: ValueKind,
    /// `#[category("Appearance")]` (WinForms `[Category]`): the Properties window group.
    pub category: Option<String>,
    /// `#[description("…")]`, else the field's doc comment.
    pub description: String,
    /// `#[default_value(…)]` as XML text (`"18"`, `"true"`, `"Round"`), empty when not given.
    pub default_value: String,
    /// `#[browsable(false)]` hides it from the Properties window (it stays settable in XML).
    pub browsable: bool,
    /// `#[property(bindable)]`: meant to be bound (`{Binding …}`), WinForms `[Bindable(true)]`.
    pub bindable: bool,
    /// `#[localizable]`.
    pub localizable: bool,
    /// `#[designer_serialization_visibility(Hidden | Visible | Content)]`.
    pub serialization: Option<String>,
    /// `#[editor("color")]`: the Properties window's editor (a colour picker…).
    pub editor: Option<String>,
    /// `#[type_converter("…")]`.
    pub type_converter: Option<String>,
    /// `#[property(on_change = "update_bar")]`: a method of the control called after the property is set (from its
    /// element's attribute, a binding or a repeater row) — the body of a Windows Forms property setter.
    pub on_change: Option<String>,
}

/// One event of a declared class (a field `#[event] name: Event<Args>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDecl {
    pub field: String,
    /// The XML attribute: `On` + the field in PascalCase (`value_committed` →
    /// `OnValueCommitted`), or `#[event(name = "…")]` (with or without the `On`).
    pub name: String,
    /// The args type's name (the last segment of `Event<Args>`'s parameter).
    pub args: String,
    /// The ⚡ tab group: one of [`EVENT_CATEGORIES`].
    pub category: &'static str,
    pub description: String,
    pub browsable: bool,
}

/// A declared class: everything the tools know of `#[derive(Component)]` /
/// `#[derive(UserControl)]` on a struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDecl {
    /// The struct (and XML element) name.
    pub name: String,
    pub kind: DeclKind,
    /// The base's name: a built-in class, a level, or another class of the project.
    pub extends: String,
    /// `levels(…)`, for a base the macro does not know.
    pub levels: Vec<String>,
    /// `overrides(…)`.
    pub overrides: Vec<String>,
    /// `#[description("…")]`, else the struct's doc comment.
    pub description: String,
    /// `#[category("…")]` / `#[toolbox(category = "…")]`: the Toolbox group.
    pub toolbox_category: Option<String>,
    /// `#[toolbox(icon = "…")]`: a Lucide glyph name or a built-in control's icon — or, WinForms'
    /// `[ToolboxBitmap]`, an image file (`#[toolbox(bitmap = "address_editor.png")]`, or an `icon`
    /// ending with an image extension), relative to the file that declares the class (see
    /// [`is_image_path`] and [`resolve_toolbox_bitmap`]).
    pub toolbox_icon: Option<String>,
    /// `#[browsable(false)]` / `#[toolbox(hidden)]`: not offered in the Toolbox.
    pub browsable: bool,
    /// `#[default_event("ValueCommitted")]`, as an attribute name (`OnValueCommitted`).
    pub default_event: Option<String>,
    /// `#[default_property("…")]`.
    pub default_property: Option<String>,
    /// `#[user_control(view = "rating_bar.kbcontrol")]`: the user control's view, relative to the
    /// file that declares it.
    pub view: Option<String>,
    pub properties: Vec<PropertyDecl>,
    pub events: Vec<EventDecl>,
}

impl ComponentDecl {
    /// The class chain as far as the built-in tables know it: `[Name, Base, …, "Component"]`
    /// for a built-in base class or a level; for a base of the project itself, `project` gives
    /// that class's chain (`None` → the chain is `[Name, Base]` followed by `levels(…)`).
    pub fn chain(&self, project: &dyn Fn(&str) -> Option<Vec<String>>) -> Vec<String> {
        let mut out = vec![self.name.clone()];
        let builtin = builtin_chain(&self.extends);
        if !builtin.is_empty() {
            out.extend(builtin.into_iter().map(str::to_string));
        } else if let Some(chain) = project(&self.extends) {
            out.extend(chain);
        } else {
            out.push(self.extends.clone());
            for l in &self.levels {
                for n in level_chain(l) {
                    if !out.iter().any(|o| o == n) {
                        out.push(n.to_string());
                    }
                }
            }
            if !out.iter().any(|o| o == "Component") {
                out.push("Component".to_string());
            }
        }
        out
    }
}

/// The ⚡ tab categories (`kubuno_desktop_views::registry::EventCategory`'s English names).
pub const EVENT_CATEGORIES: &[&str] = &["Action", "Behavior", "Data", "Drag Drop", "Focus", "Key", "Layout", "Mouse", "Property Changed", "Appearance"];

/// The event category `name` names (case-insensitive, spaces ignored: `"DragDrop"`,
/// `"drag drop"`), `"Behavior"` for anything else (WinForms' `CategoryAttribute.Default` is
/// "Misc"; the ⚡ tab has no such group).
pub fn event_category(name: Option<&str>) -> &'static str {
    let Some(name) = name else { return "Action" };
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let wanted = norm(name);
    EVENT_CATEGORIES.iter().find(|c| norm(c) == wanted).copied().unwrap_or("Behavior")
}

/// `corner_radius` → `CornerRadius`.
pub fn pascal_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = true;
    for c in name.trim_start_matches("r#").chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether a `#[toolbox(icon = …)]` value is an image file (WinForms' `[ToolboxBitmap]`) rather than
/// the name of a glyph: it ends with `.png`, `.bmp`, `.ico`, `.gif`, `.jpg` or `.jpeg`.
pub fn is_image_path(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    [".png", ".bmp", ".ico", ".gif", ".jpg", ".jpeg"].iter().any(|ext| lower.ends_with(ext))
}

/// The absolute path of a toolbox bitmap written relative to `declaring_file` (the `.rs` file of the
/// class), the way `#[user_control(view = …)]` and `include_str!` resolve; `None` when `icon` is not
/// an image file. The path is lexically normalized (`.`/`..` removed, `\` separators), so the macro
/// and the language server name the same file the same way.
pub fn resolve_toolbox_bitmap(icon: &str, declaring_file: &std::path::Path) -> Option<std::path::PathBuf> {
    if !is_image_path(icon) {
        return None;
    }
    let rel = std::path::Path::new(icon.trim());
    let joined = if rel.is_absolute() { rel.to_path_buf() } else { declaring_file.parent().unwrap_or(std::path::Path::new("")).join(rel) };
    let mut out = std::path::PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// An event's attribute name: `"Click"` → `"OnClick"`, `"OnClick"` stays.
pub fn event_attribute_name(name: &str) -> String {
    if name.len() > 2 && name.starts_with("On") && name[2..].starts_with(|c: char| c.is_ascii_uppercase()) {
        name.to_string()
    } else {
        format!("On{name}")
    }
}

// ── Reading a declaration ───────────────────────────────────────────────────────────────────

/// Helper attributes every derive of a control accepts (declare them in
/// `#[proc_macro_derive(…, attributes(…))]`).
pub const HELPER_ATTRIBUTES: &[&str] = &[
    "kubuno",
    "property",
    "event",
    "category",
    "description",
    "default_value",
    "browsable",
    "default_event",
    "default_property",
    "toolbox",
    "localizable",
    "designer_serialization_visibility",
    "editor",
    "type_converter",
    "user_control",
];

/// The doc comment of an item (`///` lines joined with spaces, trimmed).
pub fn doc_comment(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        if let Meta::NameValue(nv) = &attr.meta {
            if let Expr::Lit(lit) = &nv.value {
                if let Lit::Str(s) = &lit.lit {
                    let line = s.value();
                    let line = line.trim();
                    if !line.is_empty() {
                        lines.push(line.to_string());
                    }
                }
            }
        }
    }
    lines.join(" ")
}

/// `#[name("text")]`, `#[name = "text"]` or `#[name(Ident)]`.
fn string_arg(attr: &Attribute) -> syn::Result<String> {
    match &attr.meta {
        Meta::NameValue(nv) => match &nv.value {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => Ok(s.value()),
                other => Err(syn::Error::new(other.span(), "expected a string")),
            },
            Expr::Path(p) => Ok(path_last(&p.path)),
            other => Err(syn::Error::new(other.span(), "expected a string")),
        },
        Meta::List(list) => {
            if let Ok(s) = list.parse_args::<LitStr>() {
                return Ok(s.value());
            }
            let path: Path = list.parse_args().map_err(|_| syn::Error::new(list.span(), format!("expected `#[{}(\"…\")]`", path_last(attr.path()))))?;
            Ok(path_last(&path))
        }
        Meta::Path(p) => Err(syn::Error::new(p.span(), format!("expected a value: `#[{}(\"…\")]`", path_last(p)))),
    }
}

/// `#[name]` (true), `#[name(false)]`, `#[name = false]`.
fn bool_arg(attr: &Attribute) -> syn::Result<bool> {
    let from_expr = |e: &Expr| match e {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Bool(b) => Ok(b.value),
            other => Err(syn::Error::new(other.span(), "expected `true` or `false`")),
        },
        other => Err(syn::Error::new(other.span(), "expected `true` or `false`")),
    };
    match &attr.meta {
        Meta::Path(_) => Ok(true),
        Meta::NameValue(nv) => from_expr(&nv.value),
        Meta::List(list) => from_expr(&list.parse_args::<Expr>()?),
    }
}

/// `#[default_value(expr)]` / `#[default_value = expr]` as XML text.
fn default_value_arg(attr: &Attribute) -> syn::Result<String> {
    let expr = match &attr.meta {
        Meta::NameValue(nv) => nv.value.clone(),
        Meta::List(list) => list.parse_args::<Expr>()?,
        Meta::Path(p) => return Err(syn::Error::new(p.span(), "expected a value: `#[default_value(…)]`")),
    };
    Ok(expr_text(&expr))
}

/// The XML text of a default value: a string's content, a number without its suffix, `true`, an
/// enum variant's name, a negative number.
pub fn expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => s.value(),
            Lit::Bool(b) => b.value.to_string(),
            Lit::Int(i) => i.base10_digits().to_string(),
            Lit::Float(f) => f.base10_digits().to_string(),
            Lit::Char(c) => c.value().to_string(),
            other => quote::ToTokens::to_token_stream(other).to_string(),
        },
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => format!("-{}", expr_text(&u.expr)),
        Expr::Path(p) => path_last(&p.path),
        Expr::Paren(p) => expr_text(&p.expr),
        Expr::Group(g) => expr_text(&g.expr),
        other => quote::ToTokens::to_token_stream(other).to_string(),
    }
}

fn path_last(path: &Path) -> String {
    path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default()
}

/// A type as text, without spaces (`Option<String>`).
pub fn type_text(ty: &Type) -> String {
    quote::ToTokens::to_token_stream(ty).to_string().replace(' ', "")
}

/// The [`ValueKind`] of a Rust type (before enums of the project are resolved: see
/// [`resolve_enums`]).
pub fn value_kind(ty: &Type) -> ValueKind {
    let text = type_text(ty);
    let last = match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    match last.as_str() {
        "bool" => ValueKind::Bool,
        "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => ValueKind::Number,
        "String" => ValueKind::String,
        "Option" if text.ends_with("<String>") => ValueKind::String,
        _ if text == "&'staticstr" || text == "&str" => ValueKind::String,
        _ => ValueKind::Other(if last.is_empty() { text } else { last }),
    }
}

/// The args type of an `Event<Args>` field, `None` when the type is not `Event<…>`.
pub fn event_args(ty: &Type) -> Option<(String, Type)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Event" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => {
            let name = match t {
                Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default(),
                other => type_text(other),
            };
            Some((name, t.clone()))
        }
        _ => None,
    })
}

/// Whether an attribute is one of the design-time attributes that make a field a property.
fn is_property_marker(attr: &Attribute) -> bool {
    ["property", "category", "description", "default_value", "browsable", "localizable", "designer_serialization_visibility", "editor", "type_converter"]
        .iter()
        .any(|n| attr.path().is_ident(n))
}

/// Reads the declaration of a class from its derive input. `derive` is the derive being
/// expanded (the language server passes the one it found in `#[derive(…)]`). Errors carry the
/// span of the attribute at fault.
pub fn parse_decl(input: &DeriveInput, derive: Derive) -> syn::Result<ComponentDecl> {
    let opts = parse_kubuno_options(&input.attrs)?;
    let extends = match (&opts.extends, derive) {
        (Some(path), _) => path_last(path),
        (None, Derive::UserControl) => "UserControl".to_string(),
        (None, Derive::Component) => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "missing `#[kubuno(extends = …)]`: name the base class (`Button`, `Label`…) or level (`Control`, `ButtonBase`, `Component`…)",
            ))
        }
    };
    let mut decl = ComponentDecl {
        name: input.ident.to_string(),
        kind: DeclKind::Control,
        extends,
        levels: opts.levels.iter().map(Ident::to_string).collect(),
        overrides: opts.overrides.iter().map(Ident::to_string).collect(),
        description: doc_comment(&input.attrs),
        toolbox_category: None,
        toolbox_icon: None,
        browsable: true,
        default_event: None,
        default_property: None,
        view: None,
        properties: Vec::new(),
        events: Vec::new(),
    };

    for attr in &input.attrs {
        let p = attr.path();
        if p.is_ident("description") {
            decl.description = string_arg(attr)?;
        } else if p.is_ident("category") {
            decl.toolbox_category = Some(string_arg(attr)?);
        } else if p.is_ident("browsable") {
            decl.browsable = bool_arg(attr)?;
        } else if p.is_ident("default_event") {
            decl.default_event = Some(event_attribute_name(&string_arg(attr)?));
        } else if p.is_ident("default_property") {
            decl.default_property = Some(string_arg(attr)?);
        } else if p.is_ident("toolbox") {
            if let Meta::List(_) = &attr.meta {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("icon") || meta.path.is_ident("bitmap") {
                        let value = meta.value()?.parse::<LitStr>()?;
                        if meta.path.is_ident("bitmap") && !is_image_path(&value.value()) {
                            return Err(syn::Error::new(value.span(), "`bitmap` names an image file next to this source file (`.png`, `.bmp`, `.ico`, `.gif`, `.jpg`)"));
                        }
                        decl.toolbox_icon = Some(value.value());
                        Ok(())
                    } else if meta.path.is_ident("category") {
                        decl.toolbox_category = Some(meta.value()?.parse::<LitStr>()?.value());
                        Ok(())
                    } else if meta.path.is_ident("hidden") {
                        decl.browsable = false;
                        Ok(())
                    } else {
                        Err(meta.error("unknown `toolbox` option (expected `icon = \"…\"`, `bitmap = \"…\"`, `category = \"…\"` or `hidden`)"))
                    }
                })?;
            } else {
                decl.browsable = bool_arg(attr)?;
            }
        } else if p.is_ident("user_control") {
            if derive != Derive::UserControl {
                return Err(syn::Error::new(attr.span(), "`#[user_control(…)]` goes with `#[derive(UserControl)]`"));
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("view") {
                    decl.view = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                } else if meta.path.is_ident("default_event") {
                    decl.default_event = Some(event_attribute_name(&meta.value()?.parse::<LitStr>()?.value()));
                    Ok(())
                } else if meta.path.is_ident("toolbox_category") {
                    decl.toolbox_category = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                } else {
                    Err(meta.error("unknown `user_control` option (expected `view = \"file.kbcontrol\"`, `default_event = \"…\"` or `toolbox_category = \"…\"`)"))
                }
            })?;
        }
    }

    // The kind: a user control, else a control when `Control` is in the chain, else non-visual.
    decl.kind = if derive == Derive::UserControl {
        if decl.view.is_none() {
            return Err(syn::Error::new_spanned(&input.ident, "a user control names its view: `#[user_control(view = \"rating_bar.kbcontrol\")]`"));
        }
        DeclKind::UserControl
    } else {
        let chain = decl.chain(&|_| None);
        if chain.iter().any(|c| c == "UserControl") {
            DeclKind::UserControl
        } else if chain.iter().any(|c| c == "Control") || (builtin_chain(&decl.extends).is_empty() && decl.levels.is_empty()) {
            // An unknown base (another class of the project) without `levels(…)`: the macro
            // rejects it; the tools assume a control.
            DeclKind::Control
        } else {
            DeclKind::Component
        }
    };

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(Component)]` only supports structs with named fields"));
    };
    if let Fields::Named(fields) = &data.fields {
        for field in &fields.named {
            let Some(ident) = &field.ident else { continue };
            let field_name = ident.to_string();
            let is_event = field.attrs.iter().any(|a| a.path().is_ident("event"));
            if is_event {
                decl.events.push(parse_event(field, &field_name)?);
                continue;
            }
            if !field.attrs.iter().any(is_property_marker) {
                continue;
            }
            decl.properties.push(parse_property(field, &field_name)?);
        }
    }

    if let Some(name) = &decl.default_property {
        if !decl.properties.iter().any(|p| &p.name == name) && builtin_chain(&decl.extends).is_empty() {
            // A default property of the base class is fine; only an unknown one on a class with
            // no known base is an error the tools cannot check — left to the registry.
            let _ = name;
        }
    }
    Ok(decl)
}

fn parse_property(field: &syn::Field, field_name: &str) -> syn::Result<PropertyDecl> {
    let mut prop = PropertyDecl {
        field: field_name.to_string(),
        name: pascal_case(field_name),
        ty: type_text(&field.ty),
        kind: value_kind(&field.ty),
        category: None,
        description: doc_comment(&field.attrs),
        default_value: String::new(),
        browsable: true,
        bindable: false,
        localizable: false,
        serialization: None,
        editor: None,
        type_converter: None,
        on_change: None,
    };
    for attr in &field.attrs {
        let p = attr.path();
        if p.is_ident("property") {
            if let Meta::List(_) = &attr.meta {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("name") {
                        prop.name = meta.value()?.parse::<LitStr>()?.value();
                        Ok(())
                    } else if meta.path.is_ident("bindable") {
                        prop.bindable = true;
                        Ok(())
                    } else if meta.path.is_ident("on_change") {
                        let method = meta.value()?.parse::<LitStr>()?;
                        if syn::parse_str::<Ident>(&method.value()).is_err() {
                            return Err(syn::Error::new(method.span(), "`on_change` names a method of the control: `on_change = \"update_bar\"`"));
                        }
                        prop.on_change = Some(method.value());
                        Ok(())
                    } else {
                        Err(meta.error("unknown `property` option (expected `name = \"…\"`, `bindable` or `on_change = \"method\"`)"))
                    }
                })?;
            }
        } else if p.is_ident("category") {
            prop.category = Some(string_arg(attr)?);
        } else if p.is_ident("description") {
            prop.description = string_arg(attr)?;
        } else if p.is_ident("default_value") {
            prop.default_value = default_value_arg(attr)?;
        } else if p.is_ident("browsable") {
            prop.browsable = bool_arg(attr)?;
        } else if p.is_ident("localizable") {
            prop.localizable = bool_arg(attr)?;
        } else if p.is_ident("designer_serialization_visibility") {
            let v = string_arg(attr)?;
            if !["Visible", "Hidden", "Content"].contains(&v.as_str()) {
                return Err(syn::Error::new(attr.span(), "expected `Visible`, `Hidden` or `Content`"));
            }
            prop.serialization = Some(v);
        } else if p.is_ident("editor") {
            prop.editor = Some(string_arg(attr)?);
        } else if p.is_ident("type_converter") {
            prop.type_converter = Some(string_arg(attr)?);
        }
    }
    if !prop.name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) || !prop.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(syn::Error::new(field.span(), format!("`{}` is not a valid XML attribute name (PascalCase letters and digits)", prop.name)));
    }
    Ok(prop)
}

fn parse_event(field: &syn::Field, field_name: &str) -> syn::Result<EventDecl> {
    let Some((args, _)) = event_args(&field.ty) else {
        return Err(syn::Error::new(field.ty.span(), "an `#[event]` field is an `Event<Args>` (`kubuno_desktop_views::events::Event`)"));
    };
    let mut event = EventDecl {
        field: field_name.to_string(),
        name: event_attribute_name(&pascal_case(field_name)),
        args,
        category: "Action",
        description: doc_comment(&field.attrs),
        browsable: true,
    };
    for attr in &field.attrs {
        let p = attr.path();
        if p.is_ident("event") {
            if let Meta::List(_) = &attr.meta {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("name") {
                        event.name = event_attribute_name(&meta.value()?.parse::<LitStr>()?.value());
                        Ok(())
                    } else {
                        Err(meta.error("unknown `event` option (expected `name = \"…\"`)"))
                    }
                })?;
            }
        } else if p.is_ident("category") {
            event.category = event_category(Some(&string_arg(attr)?));
        } else if p.is_ident("description") {
            event.description = string_arg(attr)?;
        } else if p.is_ident("browsable") {
            event.browsable = bool_arg(attr)?;
        }
    }
    Ok(event)
}

/// The variants of a fieldless enum deriving `PropertyValue` (its XML values).
pub fn parse_property_enum(input: &DeriveInput) -> syn::Result<Vec<String>> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(PropertyValue)]` goes on a fieldless enum (its variants are the XML values)"));
    };
    let mut out = Vec::new();
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(v, "`#[derive(PropertyValue)]` needs variants without fields"));
        }
        out.push(v.ident.to_string());
    }
    if out.is_empty() {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(PropertyValue)]` needs at least one variant"));
    }
    Ok(out)
}

// ── Scanning source files (the language server) ─────────────────────────────────────────────

/// What [`scan_source`] finds in one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanResult {
    pub components: Vec<ComponentDecl>,
    /// `#[derive(PropertyValue)]` enums: name and variants.
    pub enums: Vec<(String, Vec<String>)>,
}

/// The derives of an item that matter here (the last path segment of each `#[derive(…)]` entry).
fn derives(attrs: &[Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("derive")) {
        let _ = attr.parse_nested_meta(|meta| {
            out.push(path_last(&meta.path));
            Ok(())
        });
    }
    out
}

fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg") && quote::ToTokens::to_token_stream(&a.meta).to_string().replace(' ', "") == "cfg(test)")
}

/// Every declared class and property enum of a Rust source file, including inline modules
/// (`mod m { … }`), skipping `#[cfg(test)]` items. A file that does not parse yields nothing (the
/// previous scan of it stays the caller's to keep); a struct whose attributes are malformed is
/// skipped (the compiler reports it).
pub fn scan_source(text: &str) -> ScanResult {
    let mut out = ScanResult::default();
    if let Ok(file) = syn::parse_file(text) {
        scan_items(&file.items, &mut out);
    }
    resolve_enums(&mut out.components, &out.enums.clone());
    out
}

/// [`scan_source`], but `None` when the file does not parse (so a caller keeps its last result).
pub fn try_scan_source(text: &str) -> Option<ScanResult> {
    let file = syn::parse_file(text).ok()?;
    let mut out = ScanResult::default();
    scan_items(&file.items, &mut out);
    let enums = out.enums.clone();
    resolve_enums(&mut out.components, &enums);
    Some(out)
}

fn scan_items(items: &[Item], out: &mut ScanResult) {
    for item in items {
        match item {
            Item::Struct(s) if !is_cfg_test(&s.attrs) => {
                let d = derives(&s.attrs);
                let derive = if d.iter().any(|n| n == "UserControl") {
                    Derive::UserControl
                } else if d.iter().any(|n| n == "Component") {
                    Derive::Component
                } else {
                    continue;
                };
                let input = DeriveInput::from(s.clone());
                if let Ok(decl) = parse_decl(&input, derive) {
                    out.components.push(decl);
                }
            }
            Item::Enum(e) if !is_cfg_test(&e.attrs) => {
                if derives(&e.attrs).iter().any(|n| n == "PropertyValue") {
                    if let Ok(variants) = parse_property_enum(&DeriveInput::from(e.clone())) {
                        out.enums.push((e.ident.to_string(), variants));
                    }
                }
            }
            Item::Mod(m) if !is_cfg_test(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    scan_items(items, out);
                }
            }
            _ => {}
        }
    }
}

/// Turns [`ValueKind::Other`] properties whose type is one of `enums` into [`ValueKind::Enum`]
/// (enums may be declared in another file: call it again with every file's enums).
pub fn resolve_enums(components: &mut [ComponentDecl], enums: &[(String, Vec<String>)]) {
    for c in components {
        for p in &mut c.properties {
            if let ValueKind::Other(ty) = &p.kind {
                if let Some((_, variants)) = enums.iter().find(|(n, _)| n == ty) {
                    p.kind = ValueKind::Enum(variants.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUND_BUTTON: &str = r#"
        use kubuno_desktop_views::prelude::*;

        /// A button drawn as a pill.
        #[derive(Component, Default)]
        #[kubuno(extends = Button, overrides(Control))]
        #[category("Kubuno")]
        #[toolbox(icon = "circle")]
        #[default_event("LongPress")]
        #[default_property("CornerRadius")]
        pub struct RoundButton {
            base: Button,
            /// The radius of the corners, in pixels.
            #[property]
            #[category("Appearance")]
            #[default_value(18.0)]
            pub corner_radius: f32,
            #[property(name = "Shape", bindable)]
            #[description("The outline.")]
            #[default_value(Shape::Pill)]
            pub shape: Shape,
            #[browsable(false)]
            pub secret: String,
            pub not_a_property: u32,
            /// Occurs when the button is held.
            #[event]
            #[category("Mouse")]
            pub long_press: Event<MouseEventArgs>,
        }

        #[derive(PropertyValue, Default, Clone, Copy)]
        pub enum Shape { #[default] Pill, Square }

        #[cfg(test)]
        mod tests {
            #[derive(Component)]
            #[kubuno(extends = Button)]
            struct Hidden { base: Button }
        }

        mod nested {
            #[derive(kubuno_desktop_views::component::Component, Default)]
            #[kubuno(extends = Control)]
            pub struct Gauge { base: ControlCore }

            #[derive(Component, Default)]
            #[kubuno(extends = Component)]
            pub struct Ticker { base: ComponentCore }
        }
    "#;

    #[test]
    fn a_file_is_scanned_with_its_design_time_attributes() {
        let scan = scan_source(ROUND_BUTTON);
        let names: Vec<_> = scan.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["RoundButton", "Gauge", "Ticker"], "cfg(test) items are skipped, inline modules are read");
        assert_eq!(scan.enums, [("Shape".to_string(), vec!["Pill".to_string(), "Square".to_string()])]);

        let rb = &scan.components[0];
        assert_eq!(rb.kind, DeclKind::Control);
        assert_eq!(rb.extends, "Button");
        assert_eq!(rb.overrides, ["Control"]);
        assert_eq!(rb.description, "A button drawn as a pill.");
        assert_eq!(rb.toolbox_category.as_deref(), Some("Kubuno"));
        assert_eq!(rb.toolbox_icon.as_deref(), Some("circle"));
        assert_eq!(rb.default_event.as_deref(), Some("OnLongPress"));
        assert_eq!(rb.default_property.as_deref(), Some("CornerRadius"));
        assert_eq!(rb.chain(&|_| None), ["RoundButton", "Button", "ButtonBase", "Control", "Component"]);

        let props: Vec<_> = rb.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(props, ["CornerRadius", "Shape", "Secret"]);
        let radius = &rb.properties[0];
        assert_eq!((radius.kind.clone(), radius.category.as_deref(), radius.default_value.as_str()), (ValueKind::Number, Some("Appearance"), "18.0"));
        assert_eq!(radius.description, "The radius of the corners, in pixels.");
        let shape = &rb.properties[1];
        assert_eq!(shape.kind, ValueKind::Enum(vec!["Pill".into(), "Square".into()]));
        assert!(shape.bindable && shape.default_value == "Pill" && shape.description == "The outline.");
        assert!(!rb.properties[2].browsable);

        assert_eq!(rb.events.len(), 1);
        let e = &rb.events[0];
        assert_eq!((e.name.as_str(), e.args.as_str(), e.category, e.description.as_str()), ("OnLongPress", "MouseEventArgs", "Mouse", "Occurs when the button is held."));

        assert_eq!(scan.components[1].kind, DeclKind::Control);
        assert_eq!(scan.components[2].kind, DeclKind::Component, "a Component-level class is non-visual");
    }

    #[test]
    fn user_controls_name_their_view() {
        let src = r#"
            /// Stars.
            #[derive(UserControl, Default)]
            #[user_control(view = "rating_bar.kbcontrol", default_event = "ValueCommitted")]
            pub struct RatingBar {
                base: UserControlCore,
                #[property(bindable)] #[default_value(5)] pub max: u32,
                #[event] pub value_committed: Event<ValueCommittedArgs>,
            }
        "#;
        let scan = scan_source(src);
        let rb = &scan.components[0];
        assert_eq!(rb.kind, DeclKind::UserControl);
        assert_eq!(rb.view.as_deref(), Some("rating_bar.kbcontrol"));
        assert_eq!(rb.default_event.as_deref(), Some("OnValueCommitted"));
        assert_eq!(rb.chain(&|_| None), ["RatingBar", "UserControl", "ContainerControl", "ScrollableControl", "Control", "Component"]);
        assert_eq!(rb.properties[0].default_value, "5");
    }

    #[test]
    fn malformed_declarations_are_errors_the_macro_reports() {
        let err = |src: &str, derive| {
            let input: DeriveInput = syn::parse_str(src).unwrap();
            parse_decl(&input, derive).unwrap_err().to_string()
        };
        assert!(err("struct A { base: Button }", Derive::Component).contains("missing `#[kubuno(extends"));
        assert!(err("#[derive(UserControl)] struct A { base: UserControlCore }", Derive::UserControl).contains("names its view"));
        assert!(err("#[kubuno(extends = Button)] #[toolbox(color = 1)] struct A { base: Button }", Derive::Component).contains("unknown `toolbox` option"));
        assert!(err("#[kubuno(extends = Button)] struct A { base: Button, #[event] e: u32 }", Derive::Component).contains("Event<Args>"));
        assert!(err("#[kubuno(extends = Button)] struct A { base: Button, #[property(name = \"lower\")] p: u32 }", Derive::Component).contains("not a valid XML attribute name"));
        assert!(err("#[kubuno(extends = Button)] struct A { base: Button, #[designer_serialization_visibility(Sometimes)] p: u32 }", Derive::Component).contains("Visible"));
        assert!(err("#[kubuno(extends = Button)] #[user_control(view = \"x\")] struct A { base: Button }", Derive::Component).contains("goes with"));
    }

    #[test]
    fn helpers() {
        assert_eq!(pascal_case("corner_radius"), "CornerRadius");
        assert_eq!(pascal_case("r#type"), "Type");
        assert_eq!(event_attribute_name("Click"), "OnClick");
        assert_eq!(event_attribute_name("OnClick"), "OnClick");
        assert_eq!(event_attribute_name("Online"), "OnOnline");
        assert_eq!(event_category(Some("drag drop")), "Drag Drop");
        assert_eq!(event_category(Some("Misc")), "Behavior");
        assert_eq!(builtin_chain("Label"), ["Label", "LabelBase", "Control", "Component"]);
        assert_eq!(builtin_chain("Timer"), ["Timer", "Component"]);
        assert!(builtin_chain("Nope").is_empty());
        let ty: Type = syn::parse_str("Option<String>").unwrap();
        assert_eq!(value_kind(&ty), ValueKind::String);
        let ty: Type = syn::parse_str("kubuno_desktop_ui::Color").unwrap();
        assert_eq!(value_kind(&ty), ValueKind::Other("Color".into()));
        let e: Expr = syn::parse_str("-1.5f32").unwrap();
        assert_eq!(expr_text(&e), "-1.5");
        assert!(try_scan_source("struct {").is_none());
    }
}
