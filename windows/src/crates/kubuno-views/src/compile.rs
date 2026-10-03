//! Parse + validate + build — `XML_VIEWS.md` §5's "arse+compile happens once
//! per file change, resolving every element against the registry into a
//! small closure plan". [`compile`] is that whole pass: lex/parse ([`crate::
//! syntax`]), validate against the registry ([`crate::validate`]), and — only
//! once the file is clean — walk the typed [`crate::ast`] tree calling each
//! element's `ComponentMeta::build` (declared once per component by the
//! `component!` table in `crate::registry::components`) to produce the live
//! [`crate::node::ViewNode`] tree [`crate::runtime::Runtime`] paints every
//! frame after.
//!
//! Named `compile.rs`, not `build.rs`, precisely so it is never mistaken for
//! a Cargo build script (§2's option B, deliberately not built in this
//! phase): this file runs at *runtime*, on every file change, not at
//! `cargo build` time.
//!
//! A file with any diagnostic — a parse error, an unknown element/attribute,
//! a bad enum/type value — never reaches `build`: validation is deliberately
//! a gate, not merely advisory, so a `build` closure can read `Props` without
//! re-deriving checks [`crate::validate`] already performed (see that
//! module's own diagnostics, which [`compile`] returns unchanged on failure).
//! [`crate::runtime::Runtime`] is what keeps painting the last good tree
//! while a file is in this broken state — this module only ever answers
//! "here is a fresh tree" or "here are the diagnostics", never both at once.

use crate::ast::{AstNode, Document, Element};
use crate::design::DesignSlot;
use crate::events::router::SlotEvents;
use crate::props::{BuildCx, BuildError, Props};
use crate::registry::{self, ComponentMeta, LayoutKind};
use crate::syntax::{self, Diagnostic, Parse};
use crate::validate;
use crate::node::ViewNode;

/// A successfully compiled `.kbview` file — the "plan" `XML_VIEWS.md` §5
/// talks about. Opaque: a caller drives it through
/// [`crate::runtime::Runtime`], not by reaching into the tree.
pub struct CompiledView {
    pub(crate) root: Box<dyn ViewNode>,
    /// The root element as the input router sees it (the sender of the view events).
    pub(crate) root_events: std::rc::Rc<crate::events::router::SlotEvents>,
    /// [`crate::design::declared_design_size`] of the compiled text.
    pub(crate) design_size: Option<(f32, f32)>,
    /// The root's `Form` properties, the `<ToolTip>` settings and the `<ContextMenu>`s of the view
    /// (`crate::window`).
    pub(crate) form: crate::window::FormSpec,
    pub(crate) tooltips: crate::window::ToolTipSettings,
    pub(crate) menus: Vec<crate::window::MenuSpec>,
    /// The view's `<Command>`s and keyboard shortcuts (`crate::menus`).
    pub(crate) commands: Vec<crate::menus::CommandSpec>,
    pub(crate) accelerators: Vec<crate::menus::Accelerator>,
    /// The named components (DATA-2, `crate::scope`), handed to the runtime's scope.
    pub(crate) components: Vec<crate::scope::Entry>,
}

/// Parses, validates and builds `text` against the default registry
/// ([`crate::registry::all`]). `Err` carries every diagnostic collected —
/// parse errors and validator findings together, exactly as
/// [`crate::validate::validate`] already returns them, plus (rare: only on an
/// internal inconsistency the validator should already have caught) a
/// build-time diagnostic.
/// The view as the designer shows it: merged over its base views (`x:Inherits`, read next to the view this thread
/// last loaded) and, in the designer, with its design-time attributes (`d:…`) applied — what the design canvas reads
/// its size, title and window style from. `text` itself when there is nothing to apply.
pub fn designed_view_text(text: &str) -> String {
    let dir = crate::icon::default_base_dir();
    let merged = kubuno_views_meta::inherit::resolve(text, dir.map(|d| d.join("view.kbview")).as_deref(), &|p| std::fs::read_to_string(p)).ok().flatten().map(|(v, _)| v);
    let text = merged.as_deref().unwrap_or(text);
    crate::design::design_time().then(|| kubuno_views_meta::inherit::apply_design_attributes(text)).flatten().unwrap_or_else(|| text.to_string())
}

pub fn compile(text: &str) -> Result<CompiledView, Vec<Diagnostic>> {
    compile_with_registry(text, registry::all())
}

/// [`compile`] for the view file in `base_dir` (the folder relative image paths are resolved
/// against).
pub fn compile_in(text: &str, base_dir: Option<&std::path::Path>) -> Result<CompiledView, Vec<Diagnostic>> {
    compile_full(text, registry::all(), base_dir, Vec::new())
}

/// [`compile_in`] keeping the instances of named components of a previous tree (a hot reload):
/// an element with the same `x:Name` and class gets the same instance (a `BindingSource` keeps
/// its rows).
pub(crate) fn compile_reusing(
    text: &str,
    base_dir: Option<&std::path::Path>,
    reuse: crate::scope::Reusable,
) -> Result<CompiledView, Vec<Diagnostic>> {
    compile_full(text, registry::all(), base_dir, reuse)
}

/// [`compile`] against an explicit registry slice — what
/// [`crate::registry::tests`] and this module's own tests use to exercise a
/// smaller table without depending on [`crate::registry::all`]'s exact
/// contents.
pub fn compile_with_registry(text: &str, reg: &[ComponentMeta]) -> Result<CompiledView, Vec<Diagnostic>> {
    compile_full(text, reg, None, Vec::new())
}

fn compile_full(
    text: &str,
    reg: &[ComponentMeta],
    base_dir: Option<&std::path::Path>,
    reuse: crate::scope::Reusable,
) -> Result<CompiledView, Vec<Diagnostic>> {
    let text = prepare_text(text, base_dir).map_err(|d| vec![d])?;
    compile_prepared(&text, reg, base_dir, reuse)
}

/// The text the compiler actually reads: the view merged over its base (`x:Inherits`) and, in the
/// designer, with its design-time attributes applied. `Err` when the base view cannot be read.
pub(crate) fn prepare_text<'t>(text: &'t str, base_dir: Option<&std::path::Path>) -> Result<std::borrow::Cow<'t, str>, Diagnostic> {
    use std::borrow::Cow;
    // Visual inheritance (`x:Inherits="base.kbview"`): the view merged over its base, read next to it — a designer
    // edit or a hot reload of a derived view (the macros embed the merged view already).
    let text: Cow<'t, str> = match kubuno_views_meta::inherit::resolve(text, base_dir.map(|d| d.join("view.kbview")).as_deref(), &|p| std::fs::read_to_string(p)) {
        Ok(Some((view, _))) => Cow::Owned(view),
        Ok(None) => Cow::Borrowed(text),
        Err(message) => return Err(Diagnostic { range: Default::default(), line: 1, column: 1, message }),
    };
    // In the designer, the design-time attributes (`d:Visible="false"`) replace their run-time ones — in the view
    // being designed only, not in the views of the user controls it uses (see `inside_user_control`).
    let design_attributes = crate::design::design_time() && !crate::node::custom::inside_user_control();
    Ok(match design_attributes.then(|| kubuno_views_meta::inherit::apply_design_attributes(&text)).flatten() {
        Some(view) => Cow::Owned(view),
        None => text,
    })
}

/// Parses, validates and builds a text [`prepare_text`] already prepared.
pub(crate) fn compile_prepared(
    text: &str,
    reg: &[ComponentMeta],
    base_dir: Option<&std::path::Path>,
    reuse: crate::scope::Reusable,
) -> Result<CompiledView, Vec<Diagnostic>> {
    let parse = syntax::parse(text);
    let mut diagnostics = parse.diagnostics.clone();
    diagnostics.extend(validate::validate(&parse, reg));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let Some(doc) = Document::cast(parse.syntax()) else {
        return Err(vec![Diagnostic { range: parse.syntax().text_range(), line: 1, column: 1, message: "not a document".to_string() }]);
    };
    let Some(root) = doc.root_element() else {
        return Err(vec![Diagnostic { range: parse.syntax().text_range(), line: 1, column: 1, message: "the view has no root element".to_string() }]);
    };

    // Relative icon files are resolved against the view's folder while it is built.
    let _icons = crate::icon::enter_base_dir(base_dir);
    crate::icon::install();
    let mut cx = BuildCx::new();
    cx.base_dir = base_dir.map(std::path::Path::to_path_buf);
    cx.reuse = reuse;
    cx.tab_order = root.syntax().descendants().filter_map(Element::cast).any(|e| e.attribute("TabIndex").is_some());
    let root_events = match root.name().and_then(|n| registry::lookup(&n)) {
        Some(meta) => std::rc::Rc::new(SlotEvents::from_element(&root, meta, true)),
        None => std::rc::Rc::new(SlotEvents::new(String::new(), "")),
    };
    let menus = crate::window::read_menus(&root);
    let commands = crate::menus::read_commands(&root);
    let has_ribbon = root.syntax().descendants().filter_map(Element::cast).any(|e| e.name().as_deref() == Some("Ribbon"));
    let accelerators = crate::menus::accelerators(&menus, &commands, has_ribbon);
    match build_node(&root, &mut cx, LayoutKind::None) {
        Ok(node) => Ok(CompiledView {
            // The floating parts (`<Popover>`) paint above the whole view, wherever they are declared.
            root: with_top_layer(node, &mut cx),
            root_events,
            design_size: crate::design::declared_design_size(Some(&doc)),
            form: crate::window::FormSpec::read(&root, base_dir),
            tooltips: crate::window::ToolTipSettings::read(&root),
            menus,
            commands,
            accelerators,
            components: std::mem::take(&mut cx.components),
        }),
        Err(e) => Err(vec![build_error_to_diagnostic(&e, &parse)]),
    }
}

/// `node` with the floating parts built with it (`<Popover>`) painted above it, wherever they are
/// declared (`crate::node::TopLayerRoot`).
pub(crate) fn with_top_layer(node: Box<dyn ViewNode>, cx: &mut BuildCx) -> Box<dyn ViewNode> {
    if cx.top_layer.is_empty() {
        return node;
    }
    Box::new(crate::node::TopLayerRoot { root: node, layer: std::mem::take(&mut cx.top_layer) })
}

thread_local! {
    /// The item being built by a `<Repeater>` (its key's hash), while its template is built: the
    /// ids of the template's elements (their design slot, router entry and focus) are made unique
    /// per item with it, so two items never share a hover, a press or the focus.
    static ITEM_SCOPE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// The prefix of the ids of the elements being built that are not in the document (the title bar's standard
    /// items, `crate::window::build_header_items`): their ids never meet a document element's.
    static ID_PREFIX: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` (building elements made by the framework, not read from the document) with their ids under `prefix`.
pub(crate) fn with_id_prefix<R>(prefix: &str, f: impl FnOnce() -> R) -> R {
    let before = ID_PREFIX.with(|p| p.replace(Some(prefix.to_string())));
    let out = f();
    ID_PREFIX.with(|p| *p.borrow_mut() = before);
    out
}

/// Runs `f` (building an item's template) with the item scope `scope`.
pub(crate) fn with_item_scope<R>(scope: Option<u64>, f: impl FnOnce() -> R) -> R {
    let before = ITEM_SCOPE.with(|s| s.replace(scope));
    let out = f();
    ITEM_SCOPE.with(|s| s.set(before));
    out
}

/// The id scope of a nested view (a user control's own view) whose element in the outer view has the (scoped) id
/// `instance`: two instances of a user control never share an id, a focus id or an accessibility id.
pub(crate) fn scope_hash(instance: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in instance.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h.max(1)
}

/// An element's id, made unique for the item being built (see `ITEM_SCOPE`).
pub(crate) fn scoped_id(id: String) -> String {
    // The framework's own elements (`with_id_prefix`): ids that never meet a document element's.
    let id = match ID_PREFIX.with(|p| p.borrow().clone()) {
        Some(prefix) => format!("{prefix}{id}"),
        None => id,
    };
    match ITEM_SCOPE.with(std::cell::Cell::get) {
        Some(scope) => format!("{id}~{scope:x}"),
        None => id,
    }
}

/// A focus id, made unique for the item being built (see `ITEM_SCOPE`).
pub(crate) fn scoped_focus(id: kubuno_ui::FocusId) -> kubuno_ui::FocusId {
    match ITEM_SCOPE.with(std::cell::Cell::get) {
        Some(scope) => kubuno_ui::FocusId(id.0 ^ scope.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29)),
        None => id,
    }
}

/// Builds one element into its live [`ViewNode`], looked up by name in
/// [`crate::registry::all`], then wraps it in a [`DesignSlot`] carrying its
/// [`Element::stable_id`] and `parent_layout` — `vskubuno/docs/DESIGNER.md`
/// §6 (DSG-6): "needs an id on every node, not just `x:Name`'d ones". Doing
/// the wrapping HERE, the single choke point every element (this crate's own
/// five, or any `registry::families::*` component) passes through, is what
/// lets DSG-6 record a frame's layout map without every component family
/// having to do it itself (see `crate::design`'s own doc).
///
/// `parent_layout` is the [`LayoutKind`] of the CONTAINER placing this
/// element — [`LayoutKind::None`] for the document root (no parent) and for
/// [`crate::registry::ChildrenModel::SingleWidget`] callers, the container's
/// own [`crate::registry::ComponentMeta::layout`] for a `List`/Dock-Anchor
/// child (see the call sites in [`crate::props::Props::build_children`]/
/// [`crate::props::Props::build_children_sized`]/[`crate::props::Props::
/// build_single_child`], and `registry::families::containers`'s `<Panel>`
/// child loop, the one call site that does not go through `Props` at all).
///
/// `pub(crate)` — reached through the `Props` helpers above by a component's
/// own `build` closure, and by [`compile_with_registry`] for the root; never
/// called with an element that was not already checked by [`crate::validate::
/// validate`] against the same registry (see this module's doc), so an
/// "unknown element" here is defensive, not an expected path.
pub(crate) fn build_node(element: &Element, cx: &mut BuildCx, parent_layout: LayoutKind) -> Result<Box<dyn ViewNode>, BuildError> {
    let name = element.name().ok_or_else(|| BuildError::new("element has no name", element.name_range()))?;
    let meta = registry::lookup(&name)
        .ok_or_else(|| BuildError::new(format!("unknown element `<{name}>`"), element.name_range()))?;
    let props = Props::new(element, meta);
    let node = (meta.build)(&props, cx)?;
    let container = meta.children != registry::ChildrenModel::None;
    // The root reads the view events (`OnLoad`…) too: it is the element without a parent element.
    let is_root = element.syntax().parent().is_none_or(|p| p.kind() != crate::syntax::SyntaxKind::ELEMENT);
    let events = std::rc::Rc::new(SlotEvents::from_element(element, meta, is_root));
    // The properties every control inherits (`crate::common`), and its place in the Tab order.
    let common = crate::common::CommonProps::read(&props, meta, cx.base_dir.as_deref())?;
    let tab_index = cx.tab_order.then(|| {
        element.attribute("TabIndex").and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).map(|v| v as i32).unwrap_or(0)
    });
    // `AutoScroll`: the node scrolls inside its box.
    let node: Box<dyn crate::node::ViewNode> =
        if crate::common::auto_scrolls(element, meta, is_root) { Box::new(crate::common::AutoScrollNode::new(node)) } else { node };
    // A control coming from the base view of an inherited view, not overridden (`x:Inherited`, see
    // `kubuno_views_meta::inherit`): it is not in the document being designed, so the designer shows it locked.
    let id = match element.attribute(kubuno_views_meta::inherit::INHERITED_ATTRIBUTE).and_then(|a| a.value()).as_deref() {
        Some("true") => format!("{}{}", crate::design::INHERITED_LOCKED_PREFIX, element.stable_id()),
        Some("inner") => format!("{}{}", crate::design::INHERITED_INNER_PREFIX, element.stable_id()),
        _ => element.stable_id(),
    };
    let mut slot = DesignSlot::new(scoped_id(id), parent_layout, node)
        .with_container(container)
        .with_events(events.clone())
        .with_common(common)
        .with_tab_index(tab_index)
        .with_meta(meta);
    // The element's class instance (EVT-7a): its events are delivered through its `on_…` methods.
    // The classes of an application or a library are also the view's named components (DATA-2,
    // `crate::scope`): every non-visual one (a `<BindingSource>`; an unnamed one is `<class>N`, the
    // n-th element of its class: `bindingSource2`), and every control with an `x:Name`. A hot reload
    // keeps the instance of a non-visual component whose element keeps its name and class.
    let linked = registry::project_info(meta.name).filter(|i| i.origin == registry::Origin::Linked);
    let non_visual = linked.is_some_and(|i| i.kind == registry::ClassKind::Component);
    let x_name = element.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty());
    let scope_name = if non_visual {
        let n = match cx.unnamed.iter_mut().find(|(c, _)| *c == meta.name) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                cx.unnamed.push((meta.name, 1));
                1
            }
        };
        Some(x_name.clone().unwrap_or_else(|| {
            let mut chars = meta.name.chars();
            let first = chars.next().map(|c| c.to_ascii_lowercase().to_string()).unwrap_or_default();
            format!("{first}{}{n}", chars.as_str())
        }))
    } else if linked.is_some() {
        x_name
    } else {
        None
    };
    // A named component or control of an application class keeps its instance across a new
    // composition of the view (a hot reload, a control added or moved from code): like a Windows
    // Forms control, it outlives a layout change — its fields (what code set through typed access,
    // a user control's data) stay.
    let reused = scope_name.as_ref().and_then(|name| {
        let i = cx.reuse.iter().position(|(n, c, _)| n == name && *c == meta.name)?;
        Some(cx.reuse.swap_remove(i).2)
    });
    let instance = reused.or_else(|| crate::controls::class_of(meta.name).and_then(|class| (class.create)()));
    // An application class (EVT-7b): the properties its built-in base element does not read are
    // set on its instance from the element's attributes (bindings resolved every frame).
    let custom: Vec<crate::design::CustomProp> = match linked {
        Some(_) => {
            let builtin_base = meta.base_chain().iter().skip(1).find_map(|n| registry::builtins().iter().find(|m| m.name == *n));
            meta.properties.iter().filter(|p| builtin_base.is_none_or(|b| b.property(p.name).is_none())).filter_map(|p| crate::design::CustomProp::read(element, p)).collect()
        }
        None => Vec::new(),
    };
    if let Some(instance) = instance {
        if let Some(name) = scope_name {
            // Named and configured now, not at its first paint: the view's `OnLoad` may use it.
            if let Ok(mut c) = instance.try_borrow_mut() {
                c.set_site(Some(crate::component::Site { name: name.clone(), design_mode: false, container: None }));
                crate::design::apply_literal_props(&mut *c, &custom);
            }
            cx.components.push(crate::scope::Entry { name, class: meta.name, instance: std::rc::Rc::downgrade(&instance), slot: events.clone() });
        }
        slot = slot.with_control(instance);
    }
    if !custom.is_empty() {
        slot = slot.with_custom_props(custom);
    }
    Ok(Box::new(slot))
}

/// Turns a [`BuildError`] into the same [`Diagnostic`] shape
/// [`crate::syntax::parse`] and [`crate::validate::validate`] produce, so a
/// caller (the preview's error banner, later the language server) has one
/// diagnostic type regardless of which pass found the problem.
pub(crate) fn build_error_to_diagnostic(e: &BuildError, parse: &Parse) -> Diagnostic {
    let range = e.range.unwrap_or_else(|| parse.syntax().text_range());
    let lc = parse.line_col(range.start());
    Diagnostic { range, line: lc.line, column: lc.column, message: e.message.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_tree_from_well_formed_xml() {
        // `kubuno_ui`'s own tests never construct a `&dyn Canvas` either (see
        // e.g. `kubuno-ui/src/buttons.rs`'s tests, which exercise pure
        // geometry helpers instead): a real `Canvas` needs a live Direct2D
        // device, which unit tests do not have. So this checks the one thing
        // reachable without one — that `compile` actually produced a tree
        // (`CompiledView.root` is `pub(crate)`, reachable from here) rather
        // than short-circuiting on the way there.
        let src = r#"<Card Title="Réglages"><Stack Direction="TopDown"><Button Text="Ok"/></Stack></Card>"#;
        let view = compile(src).unwrap();
        let _root: &dyn ViewNode = view.root.as_ref();
    }

    #[test]
    fn builds_a_tree_with_a_two_way_bound_switch() {
        let src = r#"<Switch x:Name="notifications" On="{Binding Notifications, Mode=TwoWay}"/>"#;
        assert!(compile(src).is_ok());
    }

    /// `CompiledView` (the `Ok` side) is not `Debug` — a compiled widget tree
    /// has no useful textual form and nothing else in this crate needs one —
    /// so `Result::unwrap_err` is not reachable here; this extracts the `Err`
    /// side directly instead.
    fn expect_err(src: &str) -> Vec<Diagnostic> {
        match compile(src) {
            Ok(_) => panic!("expected `{src}` to fail to compile"),
            Err(d) => d,
        }
    }

    #[test]
    fn unknown_element_is_a_diagnostic_not_a_panic() {
        let err = expect_err(r#"<Frobnicator/>"#);
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("unknown element"), "{:?}", err[0]);
    }

    #[test]
    fn bad_attribute_value_is_a_diagnostic() {
        let err = expect_err(r#"<Button Variant="Nope"/>"#);
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("Nope"), "{:?}", err[0]);
    }

    #[test]
    fn parse_errors_prevent_build() {
        let err = expect_err(r#"<Button Text="unterminated></Button>"#);
        assert!(!err.is_empty());
    }

    #[test]
    fn a_panel_with_anchor_and_dock_children_compiles() {
        // Exercises `registry::families::containers`'s `<Panel>` build
        // closure, the one call site that calls `build_node` directly rather
        // than through a `Props` helper (see that module's doc) — a
        // regression guard for DSG-6's `build_node(element, cx,
        // parent_layout)` signature change reaching every call site,
        // including this one.
        let src = r#"<Panel><Button Anchor="Left,Top" X="4" Y="4" Width="80" Height="24"/><Button Dock="Top" Height="24"/></Panel>"#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn worked_settings_snippet_from_the_design_note_compiles() {
        // `XML_VIEWS.md` §7, restricted to the five registered components —
        // the same snippet `crate::validate`'s own test parses, compiled all
        // the way to a live tree here.
        let src = r#"
            <Card Title="Réglages">
              <Stack Direction="TopDown" Gap="0">
                <Switch x:Name="notifications"
                        On="{Binding Notifications, Mode=TwoWay}"/>
                <Switch x:Name="offline" On="{Binding Offline}"
                        OnToggled="offline_toggled"/>
                <TextField x:Name="proxy" Width="320"
                           Placeholder="http://hôte:port (aucun)"
                           Text="{Binding Proxy, Mode=TwoWay}"/>
              </Stack>
            </Card>
        "#;
        assert!(compile(src).is_ok());
    }
}
