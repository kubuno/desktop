//! The in-memory composition of a form's view: its `.kbview` text (never modified on disk), with
//!
//! - the properties code reads and writes on its named controls bound to the controls' values
//!   (`Text="{Binding __kb.3.Text, Mode=TwoWay}"`), so `self.status.set_text(…)` needs no
//!   recomposition and what the user types lands in the control;
//! - every event attribute routed to the form (`OnClick="__kb5"`), which runs the handler the view
//!   named and the control's Rust subscribers, and an event attribute for each event a control has
//!   Rust subscribers for;
//! - the controls created in code, as elements, where they were added;
//!
//! serialised back to text for `kubuno_desktop_views::runtime::Runtime::reload_from_text`.

use kubuno_desktop_views::ast::{AstNode, Document, Element};
use kubuno_desktop_views::binding::{is_binding_expr, parse_binding, Value};
use kubuno_desktop_views::registry::{self, ComponentMeta, PropKind};

use super::{Anchor, Control, DockStyle, Form, Layout};

/// A handler of the composed view: `OnClick="__kb<index>"`.
#[derive(Clone)]
pub(crate) struct Synthetic {
    /// The control that raises it (the form's root for the view's own events).
    pub control: Control,
    /// Its canonical attribute name (`"OnClick"`, `"OnCheckedChanged"`).
    pub event: String,
    /// The handler the `.kbview` names (`hello_click`), if any.
    pub user: Option<String>,
}

/// The prefix of the handler names of the composed view.
pub(crate) const HANDLER_PREFIX: &str = "__kb";
/// The prefix of the binding paths to the controls' values.
pub(crate) const BINDING_PREFIX: &str = "__kb.";

/// Properties the runtime reads through a binding: bound to the control's value when the control
/// has a handle (so changing them from code costs no recomposition).
const DYNAMIC: &[&str] = &[
    "Text", "Title", "Enabled", "Visible", "Checked", "On", "Value", "Minimum", "Maximum", "SelectedIndex", "SelectedValue", "Placeholder", "ReadOnly", "Label", "Description",
    "Loading", "Indeterminate", "Invalid", "ToolTip", "Header", "Layout", "ActivePanel", "ForeColor", "BackColor", "ItemsSource", "SelectedItem", "Image",
    "Initials", "Presence", "StatusText", "IsOpen", "PageIndex", "TotalRows",
];

/// Bound even when neither the view nor code sets them (what code changes most).
const ALWAYS: &[&str] = &["Text", "Enabled", "Visible"];

/// The geometry attributes, kept in [`Layout`].
const LAYOUT: &[&str] = &["X", "Y", "Width", "Height", "Anchor", "Dock"];

/// One element of the composed view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct XNode {
    pub name: String,
    /// Attributes in order, values decoded.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
}

impl XNode {
    fn new(name: &str) -> Self {
        Self { name: name.to_string(), attrs: Vec::new(), children: Vec::new() }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name.to_string(), value)),
        }
    }

    fn from_element(el: &Element) -> Option<Self> {
        Some(Self {
            name: el.name()?,
            attrs: el.attributes().filter_map(|a| Some((a.name()?, a.value()?))).collect(),
            children: el.children().filter_map(|c| Self::from_element(&c)).collect(),
        })
    }

    /// Serialises the element and its children.
    pub fn write(&self, out: &mut String, depth: usize) {
        let indent = "  ".repeat(depth);
        out.push_str(&indent);
        out.push('<');
        out.push_str(&self.name);
        for (n, v) in &self.attrs {
            out.push(' ');
            out.push_str(n);
            out.push_str("=\"");
            escape_into(v, out);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>\n");
        } else {
            out.push_str(">\n");
            for c in &self.children {
                c.write(out, depth + 1);
            }
            out.push_str(&indent);
            out.push_str("</");
            out.push_str(&self.name);
            out.push_str(">\n");
        }
    }
}

fn escape_into(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

/// Reads a view's root element; `None` when the text does not parse (the runtime then reports
/// the errors against the original text).
pub(crate) fn parse_view(text: &str) -> Option<XNode> {
    let parse = kubuno_desktop_views::syntax::parse(text);
    if !parse.diagnostics.is_empty() {
        return None;
    }
    let doc = Document::cast(parse.syntax())?;
    XNode::from_element(&doc.root_element()?)
}

/// A number as `.kbview` writes it (`75`, `12.5`).
pub(crate) fn number(f: f32) -> String {
    if f.fract() == 0.0 && f.abs() < 1.0e9 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

fn literal(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.clone()),
        Value::F32(f) => Some(number(*f)),
        Value::Bool(b) => Some(b.to_string()),
        Value::List(_) | Value::Object(_) => None,
    }
}

/// A literal attribute value as a property value of the element's kind.
fn typed(meta: Option<&'static ComponentMeta>, name: &str, raw: &str) -> Value {
    match meta.and_then(|m| m.property(name)).map(|p| p.kind) {
        Some(PropKind::Bool) => Value::Bool(raw.trim() == "true"),
        Some(PropKind::F32) => raw.trim().parse().map(Value::F32).unwrap_or_else(|_| Value::Str(raw.to_string())),
        _ => Value::Str(raw.to_string()),
    }
}

fn is_event(name: &str) -> bool {
    name.strip_prefix("On").and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_uppercase())
}

fn canonical_event(meta: Option<&'static ComponentMeta>, attribute: &str) -> String {
    meta.and_then(|m| m.event(attribute)).map(|e| e.name.to_string()).unwrap_or_else(|| attribute.to_string())
}

/// How an element relates to its handle.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The view's root: the form.
    Root,
    /// A named element of the view.
    Named,
    /// An unnamed element of the view with handlers (a handle only to be their sender).
    Anonymous,
    /// A control created in code.
    Code,
}

struct Cx<'a> {
    form: &'a Form,
    store: Vec<Control>,
    synthetic: Vec<Synthetic>,
}

/// The result of a composition.
pub(crate) struct Composition {
    /// What the runtime compiles.
    pub text: String,
    /// The view's own text had to be used as it is (it does not parse).
    pub verbatim: bool,
}

/// Composes `form`'s view (see the module documentation) and records its controls and handlers.
pub(crate) fn compose(form: &Form) -> Composition {
    let shared = &form.shared;
    shared.dirty.set(false);
    let source = shared.source.borrow().clone();
    let mut root = match &source {
        Some(src) => match parse_view(&src.text) {
            Some(root) => root,
            None => {
                shared.store.borrow_mut().clear();
                shared.synthetic.borrow_mut().clear();
                return Composition { text: src.text.clone(), verbatim: true };
            }
        },
        None => XNode::new(&shared.root.element()),
    };
    let mut cx = Cx { form, store: Vec::new(), synthetic: Vec::new() };
    *shared.root.0.element.borrow_mut() = root.name.clone();
    let root_control = shared.root.clone();
    cx.apply(&mut root, &root_control, Kind::Root);
    cx.walk(&mut root, "");
    cx.append_code(&mut root, &root_control);
    let Cx { store, synthetic, .. } = cx;
    *shared.store.borrow_mut() = store;
    *shared.synthetic.borrow_mut() = synthetic;
    let mut text = String::new();
    root.write(&mut text, 0);
    Composition { text, verbatim: false }
}

impl Cx<'_> {
    /// The handle of an element of the view: the generated field, or one the form keeps.
    fn handle_of(&self, key: &str, element: &str) -> Control {
        let shared = &self.form.shared;
        if let Some(c) = shared.members.borrow().get(key) {
            return c.clone();
        }
        let mut hidden = shared.hidden.borrow_mut();
        let c = hidden.entry(key.to_string()).or_insert_with(|| Control::new(element)).clone();
        if !key.starts_with('#') {
            *c.0.name.borrow_mut() = key.to_string();
        }
        c
    }

    fn walk(&mut self, node: &mut XNode, path: &str) {
        for (i, child) in node.children.iter_mut().enumerate() {
            let child_path = if path.is_empty() { i.to_string() } else { format!("{path}.{i}") };
            let handle = match child.get("x:Name").map(str::to_string) {
                Some(name) => Some((self.handle_of(&name, &child.name), Kind::Named)),
                None if child.attrs.iter().any(|(n, _)| is_event(n)) => Some((self.handle_of(&format!("#{child_path}"), &child.name), Kind::Anonymous)),
                None => None,
            };
            if let Some((control, kind)) = &handle {
                *control.0.element.borrow_mut() = child.name.clone();
                control.0.from_view.set(true);
                *control.0.form.borrow_mut() = std::rc::Rc::downgrade(&self.form.shared);
                self.apply(child, control, *kind);
            }
            self.walk(child, &child_path);
            if let Some((control, _)) = &handle {
                self.append_code(child, control);
            }
        }
    }

    /// Appends the controls created in code inside `owner` to `node`.
    fn append_code(&mut self, node: &mut XNode, owner: &Control) {
        let children = owner.0.children.borrow().clone();
        for child in children {
            let mut el = XNode::new(&child.element());
            self.apply(&mut el, &child, Kind::Code);
            self.append_code(&mut el, &child);
            node.children.push(el);
        }
    }

    fn apply(&mut self, node: &mut XNode, control: &Control, kind: Kind) {
        let meta = registry::lookup(&node.name);
        let key = self.store.len();
        self.store.push(control.clone());
        control.0.bound.borrow_mut().clear();
        control.0.user_bound.borrow_mut().clear();

        if kind == Kind::Code {
            node.set("x:Name", control.get_name());
        }

        // The view's literals: the control's values (unless code set them since).
        if kind != Kind::Code && kind != Kind::Anonymous {
            let mut layout = control.0.layout.get();
            for (name, value) in node.attrs.clone() {
                // Directives and namespace declarations (`xmlns`, `xmlns:x`) are markup, not values.
                if is_event(&name) || name.starts_with("x:") || name == "xmlns" || name.starts_with("xmlns:") {
                    continue;
                }
                if is_binding_expr(&value) {
                    if let Some(spec) = parse_binding(&value) {
                        control.0.user_bound.borrow_mut().insert(name.clone(), spec.path);
                    }
                    continue;
                }
                if LAYOUT.contains(&name.as_str()) {
                    if !control.0.layout_set.get() {
                        read_layout(&mut layout, &name, &value);
                    }
                    continue;
                }
                if !control.0.code_set.borrow().contains(&name) {
                    control.0.props.borrow_mut().insert(name.clone(), typed(meta, &name, &value));
                }
            }
            control.0.layout.set(layout);
        }

        // Properties: bound to the control's value, or written as literals.
        let bindable = |p: &str| match kind {
            Kind::Root => p == "Title",
            Kind::Anonymous => false,
            _ => meta.is_some_and(|m| m.property(p).is_some()),
        };
        let code_set = control.0.code_set.borrow().clone();
        let props = control.0.props.borrow().clone();
        let user_bound = control.0.user_bound.borrow().clone();
        for p in DYNAMIC.iter().copied() {
            if user_bound.contains_key(p) || !bindable(p) {
                continue;
            }
            let wanted = node.get(p).is_some() || code_set.contains(p) || props.contains_key(p) || (kind != Kind::Root && ALWAYS.contains(&p)) || (kind == Kind::Root && p == "Title");
            if wanted {
                node.set(p, format!("{{Binding {BINDING_PREFIX}{key}.{p}, Mode=TwoWay}}"));
                control.0.bound.borrow_mut().insert(p.to_string());
            }
        }
        // A list or a Rust value (`ItemsSource`, a custom control's `Rows` / `Shared<T>` property)
        // has no XML literal: it is always bound to the control's value.
        for (name, value) in &props {
            if control.0.bound.borrow().contains(name) || user_bound.contains_key(name) || !bindable(name) {
                continue;
            }
            let structured = matches!(value, Value::List(_) | Value::Object(_)) || meta.and_then(|m| m.property(name)).is_some_and(|p| p.is_bound_only());
            if structured {
                node.set(name, format!("{{Binding {BINDING_PREFIX}{key}.{name}, Mode=TwoWay}}"));
                control.0.bound.borrow_mut().insert(name.clone());
            }
        }
        for (name, value) in &props {
            let from_code = kind == Kind::Code || code_set.contains(name);
            if !from_code || control.0.bound.borrow().contains(name) || user_bound.contains_key(name) {
                continue;
            }
            if let Some(text) = literal(value) {
                node.set(name, text);
            }
        }

        // Geometry written in code.
        if kind == Kind::Code || control.0.layout_set.get() {
            let l = control.0.layout.get();
            let mut set = |name: &str, v: Option<f32>| {
                if let Some(v) = v {
                    node.set(name, number(v));
                }
            };
            set("X", l.x);
            set("Y", l.y);
            set("Width", l.width);
            set("Height", l.height);
            if let Some(a) = l.anchor {
                node.set("Anchor", a.to_xml());
            }
            if let Some(d) = l.dock {
                node.set("Dock", d.to_xml());
            }
        }

        // Events: the view's, then those with Rust subscribers.
        let mut present = Vec::new();
        for (name, value) in node.attrs.iter_mut() {
            if !is_event(name) {
                continue;
            }
            let event = canonical_event(meta, name);
            let user = Some(value.clone()).filter(|v| !v.trim().is_empty());
            *value = format!("{HANDLER_PREFIX}{}", self.synthetic.len());
            present.push(event.clone());
            self.synthetic.push(Synthetic { control: control.clone(), event, user });
        }
        // A button with a `DialogResult` sets it when clicked: its Click reaches the form.
        let mut routed = control.subscribed_events();
        if control.get_dialog_result() != super::DialogResult::None && !routed.contains(&"OnClick") {
            routed.push("OnClick");
        }
        for event in routed {
            if present.iter().any(|e| e == event) {
                continue;
            }
            node.set(event, format!("{HANDLER_PREFIX}{}", self.synthetic.len()));
            self.synthetic.push(Synthetic { control: control.clone(), event: event.to_string(), user: None });
        }
    }
}

fn read_layout(layout: &mut Layout, name: &str, value: &str) {
    let num = || value.trim().parse::<f32>().ok();
    match name {
        "X" => layout.x = num(),
        "Y" => layout.y = num(),
        "Width" => layout.width = num(),
        "Height" => layout.height = num(),
        "Anchor" => layout.anchor = Some(Anchor::parse(value)),
        "Dock" => {
            layout.dock = Some(match value.trim() {
                "Top" => DockStyle::Top,
                "Bottom" => DockStyle::Bottom,
                "Left" => DockStyle::Left,
                "Right" => DockStyle::Right,
                "Fill" => DockStyle::Fill,
                _ => DockStyle::None,
            })
        }
        _ => {}
    }
}

/// The control a `__kb.<index>.<Property>` binding path names, and the property.
pub(crate) fn resolve_path(form: &Form, path: &str) -> Option<(Control, String)> {
    let rest = path.strip_prefix(BINDING_PREFIX)?;
    let (index, prop) = rest.split_once('.')?;
    let control = form.shared.store.borrow().get(index.parse::<usize>().ok()?)?.clone();
    Some((control, prop.to_string()))
}

/// The synthetic handler named `handler`.
pub(crate) fn synthetic(form: &Form, handler: &str) -> Option<Synthetic> {
    let index: usize = handler.strip_prefix(HANDLER_PREFIX)?.parse().ok()?;
    form.shared.synthetic.borrow().get(index).cloned()
}
