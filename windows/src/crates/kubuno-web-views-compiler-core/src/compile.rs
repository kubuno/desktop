//! The compiler walk: one `.kbview` / `.kbcontrol` text + the web registry → plan, diagnostics, and the
//! facts the TypeScript generators need (names, handlers, bindings to type-check).

use std::collections::{BTreeMap, BTreeSet};

use kubuno_desktop_views_model::{PropKindEntry, PropertyEntry};
use kubuno_desktop_views_syntax::ast::{AstNode, Attribute, Document, Element as AstElement};
use kubuno_desktop_views_syntax::binding::{binding_issues, binding_parts, is_binding_expr, parse_binding_syntax, UpdateSourceTrigger};
use kubuno_desktop_views_syntax::res::{parse_res, ResArgValue, ResSyntax};
use kubuno_desktop_views_syntax::validate::{closest, with_suggestion};
use serde::Serialize;
use serde_json::Value;

use crate::lines::{Lines, Pos};
use crate::plan::{Binding, Event, Items, Node, Plan, Prop, Res, ResArgPlan, VIEWS_ABI};
use crate::registry::{Element, EventSource, PropTarget, WebRegistry};

/// What to compile.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct CompileOptions {
    /// The view file, relative to the project root, `/`-separated (`src/NotesSettingsPage.kbview`).
    pub file: String,
    /// The code-behind module as imported from the view's folder (`./NotesSettingsPage`), `None` when the
    /// view has no code-behind (the generated module then exports a default component itself).
    pub code_behind: Option<String>,
    /// The class the code-behind exports (default: the file stem).
    pub class_name: Option<String>,
    /// Keep design-time values (`d:` attributes, `DesignWidth`/`DesignHeight`) in the plan.
    pub design: bool,
}

/// A problem of the view, positioned in its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// One `x:Name`.
#[derive(Debug, Clone, Serialize)]
pub struct NameInfo {
    pub name: String,
    pub element: String,
    pub id: String,
    /// The handle type (`Button` from `@kubuno/views`, `ElementHandle` for project controls).
    pub handle: String,
    pub at: [u32; 2],
}

/// One use of a handler.
#[derive(Debug, Clone, Serialize)]
pub struct HandlerUse {
    pub name: String,
    pub event: String,
    pub element: String,
    pub sender: String,
    pub args: String,
    pub at: [u32; 2],
    pub end: [u32; 2],
}

/// A binding to type-check against the code-behind (the check file).
#[derive(Debug, Clone)]
pub(crate) struct BindingCheck {
    pub path: String,
    /// Exact source range of the path text (`None` when the path is assembled from `Source=` + `Path=`).
    pub path_at: Option<(Pos, Pos)>,
    /// The whole attribute (fallback mapping).
    pub attr_at: (Pos, Pos),
    /// `bool`, `num`, `str`, `arr`, `any`.
    pub check: &'static str,
    pub reads: bool,
    pub writes: bool,
    /// Row variables of the enclosing templates, innermost last.
    pub rows: Vec<String>,
}

/// A template scope opened by a `Repeater`: the row variable and the binding it iterates.
#[derive(Debug, Clone)]
pub(crate) struct RowScope {
    pub var: String,
    pub items: Option<BindingCheck>,
}

pub(crate) struct Facts {
    pub names: Vec<NameInfo>,
    pub handlers: Vec<HandlerUse>,
    pub checks: Vec<BindingCheck>,
    pub rows: Vec<RowScope>,
    pub props_type: Option<(String, Pos, Pos)>,
    pub root_element: String,
    /// The root element's start tag name (where view-level problems are reported).
    pub root_at: (Pos, Pos),
}

/// Members of `View` (the runtime base class) an `x:Name` or a handler may not shadow.
pub const RESERVED_MEMBERS: &[&str] = &[
    "props",
    "dataContext",
    "use",
    "t",
    "component",
    "constructor",
    "prototype",
    "plan",
    "handles",
    "notify",
    "mounted",
    "invalidate",
    "element",
];

const JS_RESERVED: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends",
    "false", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw",
    "true", "try", "typeof", "var", "void", "while", "with", "yield", "let", "static", "implements", "interface", "package", "private",
    "protected", "public", "await", "arguments", "eval",
];

/// Whether `s` is a plain JavaScript identifier (ASCII; no reserved word).
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !JS_RESERVED.contains(&s)
}

const SIZE_CLASSES: &[&str] = &["Compact", "Medium", "Expanded"];

/// The built-in converters (VIEWS-SPEC §6.1); project converters are registered at run time.
pub const BUILTIN_CONVERTERS: &[&str] = &["Not", "IsEmpty", "IsNotEmpty", "ToUpper", "ToLower", "Trim", "Equals", "NotEquals", "BoolToText", "Count"];

pub(crate) struct Compiler<'a> {
    reg: &'a WebRegistry,
    opts: &'a CompileOptions,
    text: &'a str,
    lines: Lines<'a>,
    pub diagnostics: Vec<Diagnostic>,
    names: BTreeMap<String, String>,
    pub facts: Facts,
    handlers: BTreeSet<String>,
    tray: Vec<Node>,
    rows: Vec<String>,
    row_counter: u32,
    design_size: [Option<f64>; 2],
    /// The event map of the element being compiled (sources of two-way change events).
    current_events: BTreeMap<String, EventSource>,
}

fn kind_name(kind: &PropKindEntry) -> &'static str {
    match kind {
        PropKindEntry::Bool => "Bool",
        PropKindEntry::F32 => "F32",
        PropKindEntry::String => "String",
        PropKindEntry::Enum(_) => "Enum",
    }
}

fn offset<T: Into<u32>>(size: T) -> usize {
    size.into() as usize
}

impl<'a> Compiler<'a> {
    pub fn new(text: &'a str, reg: &'a WebRegistry, opts: &'a CompileOptions) -> Self {
        Self {
            reg,
            opts,
            text,
            lines: Lines::new(text),
            diagnostics: Vec::new(),
            names: BTreeMap::new(),
            facts: Facts { names: Vec::new(), handlers: Vec::new(), checks: Vec::new(), rows: Vec::new(), props_type: None, root_element: String::new(), root_at: (Pos { line: 1, column: 1 }, Pos { line: 1, column: 1 }) },
            handlers: BTreeSet::new(),
            tray: Vec::new(),
            rows: Vec::new(),
            row_counter: 0,
            design_size: [None, None],
            current_events: BTreeMap::new(),
        }
    }

    fn pos(&self, offset: usize) -> Pos {
        self.lines.pos(offset)
    }

    fn at(&self, offset: usize) -> [u32; 2] {
        let p = self.pos(offset);
        [p.line, p.column]
    }

    fn diag(&mut self, severity: &'static str, code: &'static str, range: (usize, usize), message: String) {
        let s = self.pos(range.0);
        let e = self.pos(range.1.max(range.0));
        self.diagnostics.push(Diagnostic { severity, code, message, line: s.line, column: s.column, end_line: e.line, end_column: e.column });
    }

    fn error(&mut self, code: &'static str, range: (usize, usize), message: String) {
        self.diag("error", code, range, message);
    }

    fn warning(&mut self, code: &'static str, range: (usize, usize), message: String) {
        self.diag("warning", code, range, message);
    }

    fn element_range(el: &AstElement) -> (usize, usize) {
        match el.name_range() {
            Some(r) => (offset(r.start()), offset(r.end())),
            None => {
                let r = el.syntax().text_range();
                (offset(r.start()), offset(r.start()))
            }
        }
    }

    fn attr_range(attr: &Attribute) -> (usize, usize) {
        let r = attr.syntax().text_range();
        (offset(r.start()), offset(r.end()))
    }

    fn attr_name_range(attr: &Attribute) -> (usize, usize) {
        match attr.name_range() {
            Some(r) => (offset(r.start()), offset(r.end())),
            None => Self::attr_range(attr),
        }
    }

    /// The raw text between the quotes, and where it starts.
    fn raw_inner(attr: &Attribute) -> Option<(String, usize)> {
        let range = attr.value_range()?;
        let raw = attr.raw_value()?;
        if raw.len() < 2 {
            return None;
        }
        Some((raw[1..raw.len() - 1].to_string(), offset(range.start())))
    }

    /// Runs the walk. Returns the plan when the view has a root element.
    pub fn run(&mut self) -> Option<Plan> {
        let parse = kubuno_desktop_views_syntax::syntax::parse(self.text);
        for d in &parse.diagnostics {
            let r = (offset(d.range.start()), offset(d.range.end()));
            self.error("syntax", r, d.message.clone());
        }
        for problem in self.reg.problems.clone() {
            self.error("registry", (0, 0), problem);
        }
        let doc = Document::cast(parse.syntax())?;
        let Some(root) = doc.root_element() else {
            self.error("no-root", (0, 0), "the view has no root element".into());
            return None;
        };
        let root_name = root.name().unwrap_or_default();
        self.facts.root_element = root_name.clone();
        let rr = Self::element_range(&root);
        self.facts.root_at = (self.pos(rr.0), self.pos(rr.1));
        let is_control_file = self.opts.file.to_ascii_lowercase().ends_with(".kbcontrol");
        let is_uc_root = root_name == "UserControl";
        if is_control_file != is_uc_root && !root_name.is_empty() {
            let r = Self::element_range(&root);
            if is_uc_root {
                self.warning("view-file-kind", r, "a view whose root is `UserControl` is a user control: rename the file to `.kbcontrol`".into());
            } else {
                self.warning("view-file-kind", r, "a `.kbcontrol` holds a user control: its root must be `UserControl` (or rename the file to `.kbview`)".into());
            }
        }
        let node = self.element(&root, None, true)?;
        self.facts.names.sort_by_key(|n| (n.at[0], n.at[1]));
        self.facts.handlers.sort_by_key(|h| (h.at[0], h.at[1]));
        let design_size = match self.design_size {
            [Some(w), Some(h)] if self.opts.design => Some([w, h]),
            _ => None,
        };
        Some(Plan {
            abi: VIEWS_ABI,
            file: self.opts.file.clone(),
            kind: if is_control_file { "control" } else { "view" },
            root: node,
            tray: std::mem::take(&mut self.tray),
            names: self.names.clone(),
            handlers: self.handlers.iter().cloned().collect(),
            design_size,
        })
    }

    /// Compiles one element. `parent` is the parent element's registry entry (for items and slots).
    fn element(&mut self, el: &AstElement, parent: Option<&Element>, is_root: bool) -> Option<Node> {
        let name = el.name().unwrap_or_default();
        let range = Self::element_range(el);
        if let (Some(end), Some(start)) = (el.end_name_token(), el.name_token()) {
            if end.text() != start.text() {
                let r = end.text_range();
                self.error("end-tag", (offset(r.start()), offset(r.end())), format!("closing tag `</{}>` does not match `<{name}>`", end.text()));
            }
        }
        if name.contains('.') {
            self.error(
                "property-element",
                range,
                format!("`<{name}>` is a property element: it must be placed directly inside `<{}>`", name.split('.').next().unwrap_or_default()),
            );
            return None;
        }
        let Some(meta) = self.reg.get(&name) else {
            let suggestion = closest(&name, self.reg.names()).map(str::to_string);
            self.error("unknown-element", range, with_suggestion(format!("unknown element `{name}` on the web target"), suggestion));
            return None;
        };
        let meta = meta.clone();
        if meta.is_item() {
            let consumed = parent.and_then(|p| p.web.children_to_prop.as_ref()).is_some_and(|a| a.item.contains(&name))
                || parent.is_some_and(|p| p.entry.name == name && p.web.item_of.contains(&name));
            if !consumed {
                self.error(
                    "item-placement",
                    range,
                    format!("`<{name}>` is an item of {}: it can only be a child of one of these", meta.web.item_of.join(", ")),
                );
                return None;
            }
        }
        let id = el.stable_id();
        let mut node = Node { id: id.clone(), el: name.clone(), at: self.at(range.0), ..Node::default() };
        // Alternate component chosen by literal property values (`TextField Variant="Outlined"`).
        let alternate = meta.web.alternates.iter().find(|alt| {
            alt.when.iter().all(|(p, v)| el.attribute(p).and_then(|a| a.value()).as_deref() == Some(v.as_str()))
        });
        let (prop_map, event_map) = match alternate {
            Some(alt) => {
                node.m = alt.module.clone();
                node.x = alt.export.clone();
                node.dom = alt.dom_root.clone();
                node.fixed = alt.fixed.clone();
                (alt.prop_map.clone(), alt.event_map.clone())
            }
            None => {
                node.m = meta.web.module.clone();
                node.x = meta.web.export.clone();
                node.dom = meta.web.dom_root.clone();
                node.fixed = meta.web.fixed.clone();
                (meta.web.prop_map.clone(), meta.web.event_map.clone())
            }
        };
        self.current_events = event_map.clone();
        if meta.is_user_control() {
            node.kind = Some("user_control");
        }
        node.template = meta.web.template;
        let handle = if meta.origin == crate::registry::Origin::Host { name.clone() } else { "ElementHandle".to_string() };

        // ── Attributes ──
        let mut template_items: Option<BindingCheck> = None;
        for attr in el.attributes() {
            let Some(aname) = attr.name() else { continue };
            let arange = Self::attr_name_range(&attr);
            let value = attr.value().unwrap_or_default();
            if aname == "xmlns" || aname.starts_with("xmlns:") {
                continue;
            }
            if let Some(directive) = aname.strip_prefix("x:") {
                match directive {
                    "Name" => self.x_name(&attr, &value, &name, &id, &handle, &mut node),
                    "Props" if is_root => {
                        let full = Self::attr_range(&attr);
                        if is_identifier(&value) {
                            self.facts.props_type = Some((value.clone(), self.pos(full.0), self.pos(full.1)));
                        } else {
                            self.error("x-props", arange, format!("`x:Props` must name a TypeScript type exported by the code-behind, found `{value}`"));
                        }
                    }
                    "Props" => self.error("x-props", arange, "`x:Props` is only allowed on the view's root".into()),
                    "Inherits" => self.warning("unsupported", arange, "`x:Inherits` (inherited views) is not supported on the web target yet; it is ignored".into()),
                    other => self.warning("unknown-directive", arange, format!("unknown directive `x:{other}`; it is ignored")),
                }
                continue;
            }
            if let Some(prop) = aname.strip_prefix("d:") {
                if prop == "DataContext" {
                    continue; // `{SampleData file.json}`: read by the designer (WV-10).
                }
                match meta.property(prop) {
                    Some(p) if self.opts.design => {
                        let p = p.clone();
                        if let Some(compiled) = self.property(&attr, &p, &prop_map, &value, is_root, false) {
                            node.design.push(compiled);
                        }
                    }
                    Some(_) => {}
                    None if meta.is_user_control() => {}
                    None => {
                        let suggestion = closest(prop, meta.entry.properties.iter().map(|p| p.name.as_str())).map(str::to_string);
                        self.warning("unknown-attribute", arange, with_suggestion(format!("`{name}` has no property `{prop}`"), suggestion));
                    }
                }
                continue;
            }
            if aname == "DesignWidth" || aname == "DesignHeight" {
                if !is_root {
                    self.warning("design-size", arange, format!("`{aname}` only applies to the view's root"));
                } else {
                    match value.trim().parse::<f64>() {
                        Ok(v) => self.design_size[usize::from(aname == "DesignHeight")] = Some(v),
                        Err(_) => self.error("value", arange, format!("`{aname}` expects a number, found `{value}`")),
                    }
                }
                continue;
            }
            // Size-class value: `Direction.Expanded`.
            if let Some((base, class)) = aname.rsplit_once('.') {
                if SIZE_CLASSES.contains(&class) {
                    match meta.property(base) {
                        Some(p) if !p.root_only && !p.design_time => {
                            let p = p.clone();
                            if let Some(compiled) = self.property(&attr, &p, &prop_map, &value, is_root, true) {
                                node.sc.entry(class.to_string()).or_default().push(compiled);
                            }
                        }
                        Some(_) => self.error("size-class", arange, format!("`{base}` cannot take a size-class value")),
                        None => self.error("unknown-attribute", arange, format!("`{name}` has no property `{base}`")),
                    }
                    continue;
                }
            }
            if let Some(ev) = meta.entry.event(&aname).cloned() {
                if ev.root_only && !is_root {
                    self.error("root-only", arange, format!("`{}` is an event of the view's root", ev.name));
                    continue;
                }
                let Some(source) = event_map.get(&ev.name).cloned() else {
                    self.warning("not-on-web", arange, format!("`{}.{}` is not available on the web target; it is ignored", name, ev.name));
                    continue;
                };
                let handler = value.trim().to_string();
                if !is_identifier(&handler) || RESERVED_MEMBERS.contains(&handler.as_str()) {
                    self.error("handler-name", Self::attr_range(&attr), format!("`{handler}` is not a valid handler name (a method of the code-behind)"));
                    continue;
                }
                let full = Self::attr_range(&attr);
                let at = self.at(full.0);
                let end = self.at(full.1);
                self.handlers.insert(handler.clone());
                self.facts.handlers.push(HandlerUse {
                    name: handler.clone(),
                    event: ev.name.clone(),
                    element: name.clone(),
                    sender: handle.clone(),
                    args: ev.args_type.clone(),
                    at,
                    end,
                });
                node.events.push(Event { n: ev.name.clone(), h: handler, from: source, args_type: ev.args_type.clone(), at });
                continue;
            }
            match meta.property(&aname) {
                Some(p) => {
                    let p = p.clone();
                    let is_items = meta.web.template && p.name == "ItemsSource";
                    if let Some(compiled) = self.property(&attr, &p, &prop_map, &value, is_root, !is_items) {
                        if meta.web.template && p.name == "ItemsSource" {
                            if let Some(b) = &compiled.b {
                                template_items = Some(self.binding_check(&attr, b, "arr", true, false));
                            }
                        }
                        node.props.push(compiled);
                    }
                }
                None if meta.is_user_control() => {
                    // A user control's own properties come from its code-behind (WV-7 reads them); until then
                    // every attribute is passed through as a prop of `this.props`.
                    let to = PropTarget { prop: Some(lower_first(&aname)), ..PropTarget::default() };
                    let fake = PropertyEntry { name: aname.clone(), bindable: true, ..PropertyEntry::default() };
                    if let Some(mut compiled) = self.property(&attr, &fake, &BTreeMap::from([(aname.clone(), to)]), &value, is_root, true) {
                        compiled.n = aname.clone();
                        node.props.push(compiled);
                    }
                }
                None => {
                    let candidates = meta.entry.properties.iter().map(|p| p.name.as_str()).chain(meta.entry.events.iter().map(|e| e.name.as_str()));
                    let suggestion = closest(&aname, candidates).map(str::to_string);
                    self.error("unknown-attribute", arange, with_suggestion(format!("`{name}` has no property or event `{aname}`"), suggestion));
                }
            }
        }

        // ── Children ──
        let mut slot_children: Vec<AstElement> = Vec::new();
        let mut children: Vec<AstElement> = Vec::new();
        for child in el.children() {
            if child.name().is_some_and(|n| n.contains('.')) {
                slot_children.push(child);
            } else {
                children.push(child);
            }
        }
        if !el.text().trim().is_empty() {
            self.warning("text-content", range, format!("text inside `<{name}>` is ignored: set a property (`Text=\"…\"`) instead"));
        }
        for slot in slot_children {
            let sname = slot.name().unwrap_or_default();
            let srange = Self::element_range(&slot);
            let (owner, prop) = sname.split_once('.').unwrap_or_default();
            if owner != name {
                self.error("property-element", srange, format!("`<{sname}>` must be placed directly inside `<{owner}>`"));
                continue;
            }
            let Some(target) = meta.web.slots.get(prop).cloned() else {
                self.error("property-element", srange, format!("`{name}` has no content property `{prop}`"));
                continue;
            };
            let mut nodes = Vec::new();
            for c in slot.children() {
                if let Some(n) = self.child(&c, &meta) {
                    nodes.push(n);
                }
            }
            node.slots.insert(target, nodes);
        }

        if meta.web.template {
            self.row_counter += 1;
            let var = format!("__row{}", self.row_counter);
            self.facts.rows.push(RowScope { var: var.clone(), items: template_items.map(|mut c| {
                c.rows = self.rows.clone();
                c
            }) });
            self.rows.push(var);
        }
        let adapter = meta.web.children_to_prop.clone();
        let mut rendered: Vec<Node> = Vec::new();
        let mut items: Vec<Node> = Vec::new();
        for c in &children {
            let cname = c.name().unwrap_or_default();
            let is_adapter_item = adapter.as_ref().is_some_and(|a| a.item.contains(&cname));
            let is_nested_item = meta.is_item() && meta.web.item_of.contains(&cname) && cname == name;
            if let Some(n) = self.child(c, &meta) {
                if is_adapter_item {
                    items.push(n);
                } else {
                    if !is_nested_item && self.reg.get(&cname).is_some_and(|m| m.entry.non_visual) {
                        self.tray.push(n);
                        continue;
                    }
                    rendered.push(n);
                }
            }
        }
        if meta.web.template {
            self.rows.pop();
        }
        // Children model.
        let visual_children = rendered.len() + items.len();
        match meta.entry.children {
            kubuno_desktop_views_model::schema::ChildrenModelJson::None if visual_children > 0 => {
                self.error("children", range, format!("`<{name}>` takes no child element"));
            }
            kubuno_desktop_views_model::schema::ChildrenModelJson::SingleWidget if rendered.len() > 1 => {
                self.error("children", range, format!("`<{name}>` takes a single child element (wrap several in a layout container)"));
            }
            _ => {}
        }
        if !meta.entry.allowed_children.is_empty() {
            for c in &children {
                let cname = c.name().unwrap_or_default();
                if !meta.entry.allowed_children.contains(&cname) && self.reg.get(&cname).is_some() {
                    let r = Self::element_range(c);
                    self.error("children", r, format!("`<{name}>` only accepts {} as children", meta.entry.allowed_children.join(", ")));
                }
            }
        }
        if let Some(a) = adapter {
            node.items = Some(Items {
                prop: a.prop.clone(),
                shape: a.shape.clone().unwrap_or_else(|| "array".to_string()),
                content: a.content.clone(),
                key: a.key.clone(),
                nested: a.nested.clone(),
                list: items,
            });
        }
        if !rendered.is_empty() {
            if meta.is_item() || meta.web.template {
                // An item's own content (placed by its parent's adapter) or a template.
            } else if let Some(content) = meta.web.content.clone() {
                node.content = Some(content);
            } else if meta.entry.children != kubuno_desktop_views_model::schema::ChildrenModelJson::None {
                self.error("children", range, format!("`<{name}>` cannot host child elements on the web target yet"));
            }
            node.children = rendered;
        }
        Some(node)
    }

    fn child(&mut self, c: &AstElement, parent: &Element) -> Option<Node> {
        self.element(c, Some(parent), false)
    }

    fn x_name(&mut self, attr: &Attribute, value: &str, element: &str, id: &str, handle: &str, node: &mut Node) {
        let range = Self::attr_range(attr);
        if !is_identifier(value) {
            self.error("x-name", range, format!("`x:Name` must be an identifier (letters, digits, `_`), found `{value}`"));
            return;
        }
        if RESERVED_MEMBERS.contains(&value) {
            self.error("x-name", range, format!("`{value}` is a member of the view base class; choose another `x:Name`"));
            return;
        }
        if self.names.contains_key(value) {
            self.error("x-name", range, format!("`x:Name=\"{value}\"` is used twice in this view"));
            return;
        }
        self.names.insert(value.to_string(), id.to_string());
        let at = self.at(range.0);
        self.facts.names.push(NameInfo { name: value.to_string(), element: element.to_string(), id: id.to_string(), handle: handle.to_string(), at });
        node.name = Some(value.to_string());
    }

    /// Compiles one property attribute.
    fn property(
        &mut self,
        attr: &Attribute,
        p: &PropertyEntry,
        prop_map: &BTreeMap<String, PropTarget>,
        value: &str,
        is_root: bool,
        check: bool,
    ) -> Option<Prop> {
        let arange = Self::attr_name_range(attr);
        if p.root_only && !is_root {
            self.error("root-only", arange, format!("`{}` only applies to the view's root", p.name));
            return None;
        }
        let Some(to) = prop_map.get(&p.name).cloned() else {
            self.warning("not-on-web", arange, format!("`{}` is not available on the web target; it is ignored", p.name));
            return None;
        };
        let at = self.at(Self::attr_range(attr).0);
        let kind = kind_name(&p.kind);
        let mut out = Prop { n: p.name.clone(), to: to.clone(), v: None, b: None, res: None, kind, at };
        let trimmed = value.trim();
        if is_binding_expr(trimmed) {
            if let Some(inner) = trimmed.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                if let Some(res) = parse_res(inner) {
                    let args = self.res_args(attr, &res, check);
                    out.res = Some(Res { key: res.key, set: res.set, args });
                    return Some(out);
                }
            }
            let Some(spec) = parse_binding_syntax(trimmed) else {
                self.error("binding", Self::attr_range(attr), format!("`{trimmed}` is not a valid binding: expected `{{Binding Path[, Mode=…]}}` or `{{Res key}}`"));
                return None;
            };
            if let Some((raw, start)) = Self::raw_inner(attr) {
                for issue in binding_issues(&raw) {
                    self.warning("binding", (start + issue.range.start, start + issue.range.end), issue.message);
                }
            }
            if !spec.path.split('.').all(is_identifier) {
                self.error("binding-path", Self::attr_range(attr), format!("`{}` is not a member path (`a.b.c`; expressions are not supported, use a getter)", spec.path));
                return None;
            }
            if let Some(conv) = &spec.converter {
                if !is_identifier(conv) {
                    self.error("binding", Self::attr_range(attr), format!("`{conv}` is not a converter name"));
                }
            }
            let path_at = self.path_position(attr, &spec.path);
            let b = Binding {
                path: spec.path.clone(),
                mode: spec.mode.name(),
                trigger: (spec.update_trigger != UpdateSourceTrigger::PropertyChanged).then(|| spec.update_trigger.name()),
                conv: spec.converter.clone(),
                param: spec.converter_parameter.clone(),
                fallback: spec.fallback_value.clone(),
                format: spec.format.format_string.clone(),
                null: spec.format.null_value.clone(),
                culture: spec.format.culture.clone(),
                depth: self.rows.len() as u32,
                at: path_at.map(|(s, _)| [s.line, s.column]).unwrap_or(at),
            };
            if spec.mode.writes_back() && to.change.is_none() && to.runtime.is_none() {
                self.warning(
                    "two-way",
                    arange,
                    format!("`{}` reports no user change on the web: a `{}` binding only reads", p.name, spec.mode.name()),
                );
            }
            if check && to.convert.as_deref() != Some("binding-cell") {
                let kind_check = Self::check_kind(p, &to, spec.converter.is_some() || spec.format.format_string.is_some());
                let c = self.binding_check(attr, &b, kind_check, spec.mode.reads_source(), spec.mode.writes_back() && to.change.is_some());
                self.facts.checks.push(c);
            }
            if spec.mode.writes_back() {
                if let Some(ch) = &to.change {
                    out.to.change_from = self.current_events.get(ch).cloned();
                }
            }
            out.b = Some(b);
            return Some(out);
        }
        match literal(&p.kind, trimmed, value) {
            Ok(v) => {
                let mapped = match (&to.values, &v) {
                    (Some(values), Value::String(s)) => values.get(s).cloned().unwrap_or(v),
                    _ => v,
                };
                out.v = Some(mapped);
                if check && to.convert.as_deref() == Some("method") && is_identifier(trimmed) {
                    let full = Self::attr_range(attr);
                    let path_at = attr.value_range().map(|r| (self.pos(offset(r.start())), self.pos(offset(r.end()))));
                    self.facts.checks.push(BindingCheck { path: trimmed.to_string(), path_at, attr_at: (self.pos(full.0), self.pos(full.1)), check: "fn", reads: true, writes: false, rows: Vec::new() });
                }
                // A literal mapped through `values` is final; the runtime does not map it again.
                out.to.values = None;
                Some(out)
            }
            Err(message) => {
                let r = attr.value_range().map(|r| (offset(r.start()), offset(r.end()))).unwrap_or(arange);
                self.error("value", r, format!("`{}`: {message}", p.name));
                None
            }
        }
    }

    fn check_kind(p: &PropertyEntry, to: &PropTarget, converted: bool) -> &'static str {
        if converted {
            return "any";
        }
        match to.convert.as_deref() {
            Some("items-source") => return "arr",
            Some("equals-value" | "binding-cell" | "element-ref" | "gradient-css" | "workspace-theme" | "status-text") => return "any",
            Some("icon-node" | "icon-component") => return "str",
            _ => {}
        }
        // A property whose value is a list or an object (a custom control's `Apps`, `User`…): its registry kind
        // is `String` (the desktop's kinds have no list), its editor says what it holds.
        match p.editor.as_deref() {
            Some("list") => return "arr",
            Some("object") => return "any",
            _ => {}
        }
        match p.kind {
            PropKindEntry::Bool => "bool",
            PropKindEntry::F32 => "num",
            PropKindEntry::String | PropKindEntry::Enum(_) => "str",
        }
    }

    /// The source range of the binding's path text, when it is written in one piece.
    fn path_position(&self, attr: &Attribute, path: &str) -> Option<(Pos, Pos)> {
        let (raw, start) = Self::raw_inner(attr)?;
        let parts = binding_parts(&raw)?;
        let part = parts.iter().enumerate().find(|(i, p)| (p.key.is_none() && *i == 0) || p.key.as_deref() == Some("Path")).map(|(_, p)| p)?;
        if part.value != path {
            return None;
        }
        Some((self.pos(start + part.value_range.start), self.pos(start + part.value_range.end)))
    }

    /// The arguments of a `{Res}` (WV-6): each `{Binding …}` compiled like a one-way property binding (path
    /// validation, depth inside templates, `at`, a `__any(…)` check), each literal kept as text; problems are
    /// reported on the argument.
    fn res_args(&mut self, attr: &Attribute, res: &ResSyntax, check: bool) -> Vec<ResArgPlan> {
        let whole = Self::attr_range(attr);
        // Source positions: the same expression parsed from the raw text (entities undecoded), when its
        // arguments line up with the decoded ones.
        let raw = Self::raw_inner(attr).and_then(|(raw, start)| {
            let open = raw.find('{')?;
            let close = raw.rfind('}')?;
            let parsed = parse_res(raw.get(open + 1..close)?)?;
            let same = parsed.args.len() == res.args.len() && parsed.args.iter().zip(&res.args).all(|(a, b)| a.name == b.name) && parsed.issues.len() == res.issues.len();
            same.then_some((parsed, start + open + 1))
        });
        let span = |r: &std::ops::Range<usize>, i: usize, args: bool| -> (usize, usize) {
            match &raw {
                Some((p, base)) => {
                    let r = if args { p.args.get(i).map(|a| a.range.clone()) } else { p.issues.get(i).map(|x| x.range.clone()) }.unwrap_or_else(|| r.clone());
                    (base + r.start, base + r.end)
                }
                None => whole,
            }
        };
        for (i, issue) in res.issues.iter().enumerate() {
            let at = span(&issue.range, i, false);
            if issue.message.contains("given twice") {
                self.warning("binding", at, issue.message.clone());
            } else {
                self.error("binding", at, issue.message.clone());
            }
        }
        let mut out = Vec::new();
        for (i, arg) in res.args.iter().enumerate() {
            let arange = span(&arg.range, i, true);
            match &arg.value {
                ResArgValue::Literal(v) => out.push(ResArgPlan { n: arg.name.clone(), b: None, v: Some(v.clone()) }),
                ResArgValue::Binding(text) => {
                    // Where the nested binding's text starts in the source.
                    let value_start = raw.as_ref().and_then(|(p, base)| p.args.get(i).map(|a| base + a.value_range.start));
                    let Some(spec) = parse_binding_syntax(text) else {
                        self.error("binding", arange, format!("`{text}` is not a valid binding: expected `{{Binding Path[, …]}}`"));
                        continue;
                    };
                    if let Some(vs) = value_start {
                        for issue in binding_issues(text) {
                            self.warning("binding", (vs + issue.range.start, vs + issue.range.end), issue.message);
                        }
                    }
                    let path_range = value_start.and_then(|vs| {
                        let parts = binding_parts(text)?;
                        let part = parts.iter().enumerate().find(|(j, p)| (p.key.is_none() && *j == 0) || p.key.as_deref() == Some("Path")).map(|(_, p)| p)?;
                        (part.value == spec.path).then(|| (vs + part.value_range.start, vs + part.value_range.end))
                    });
                    if !spec.path.split('.').all(is_identifier) {
                        self.error("binding-path", path_range.unwrap_or(arange), format!("`{}` is not a member path (`a.b.c`; expressions are not supported, use a getter)", spec.path));
                        continue;
                    }
                    if spec.mode.writes_back() {
                        self.warning("binding", arange, format!("the argument `{}` only reads: `Mode={}` is ignored", arg.name, spec.mode.name()));
                    }
                    if let Some(conv) = &spec.converter {
                        if !is_identifier(conv) {
                            self.error("binding", arange, format!("`{conv}` is not a converter name"));
                        }
                    }
                    let b = Binding {
                        path: spec.path.clone(),
                        mode: kubuno_desktop_views_syntax::binding::BindingMode::OneWay.name(),
                        trigger: None,
                        conv: spec.converter.clone(),
                        param: spec.converter_parameter.clone(),
                        fallback: spec.fallback_value.clone(),
                        format: spec.format.format_string.clone(),
                        null: spec.format.null_value.clone(),
                        culture: spec.format.culture.clone(),
                        depth: self.rows.len() as u32,
                        at: self.at(path_range.map(|r| r.0).unwrap_or(arange.0)),
                    };
                    if check {
                        self.facts.checks.push(BindingCheck {
                            path: b.path.clone(),
                            path_at: path_range.map(|(s, e)| (self.pos(s), self.pos(e))),
                            attr_at: (self.pos(arange.0), self.pos(arange.1)),
                            check: "any",
                            reads: true,
                            writes: false,
                            rows: self.rows.clone(),
                        });
                    }
                    out.push(ResArgPlan { n: arg.name.clone(), b: Some(b), v: None });
                }
            }
        }
        out
    }

    fn binding_check(&self, attr: &Attribute, b: &Binding, check: &'static str, reads: bool, writes: bool) -> BindingCheck {
        let full = Self::attr_range(attr);
        BindingCheck {
            path: b.path.clone(),
            path_at: self.path_position(attr, &b.path),
            attr_at: (self.pos(full.0), self.pos(full.1)),
            check,
            reads,
            writes,
            rows: self.rows.clone(),
        }
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A literal attribute value typed by the property's kind.
fn literal(kind: &PropKindEntry, trimmed: &str, raw: &str) -> Result<Value, String> {
    match kind {
        PropKindEntry::Bool => match trimmed {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            other => Err(format!("expected `true` or `false`, found `{other}`")),
        },
        PropKindEntry::F32 => {
            if trimmed.is_empty() {
                return Ok(Value::Null);
            }
            let n: f64 = trimmed.parse().map_err(|_| format!("expected a number, found `{trimmed}`"))?;
            serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(|| format!("`{trimmed}` is not a finite number"))
        }
        PropKindEntry::String => Ok(Value::String(raw.to_string())),
        PropKindEntry::Enum(values) => {
            if values.iter().any(|v| v == trimmed) {
                Ok(Value::String(trimmed.to_string()))
            } else {
                let suggestion = closest(trimmed, values.iter().map(String::as_str)).map(str::to_string);
                Err(with_suggestion(format!("`{trimmed}` is not valid here; expected one of: {}", values.join(", ")), suggestion))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert!(is_identifier("save_click"));
        assert!(is_identifier("$x"));
        assert!(!is_identifier("1a"));
        assert!(!is_identifier("class"));
        assert!(!is_identifier("a-b"));
    }

    #[test]
    fn literals() {
        assert_eq!(literal(&PropKindEntry::Bool, "true", "true"), Ok(Value::Bool(true)));
        assert!(literal(&PropKindEntry::Bool, "yes", "yes").is_err());
        assert_eq!(literal(&PropKindEntry::F32, "12.5", "12.5"), Ok(serde_json::json!(12.5)));
        assert_eq!(literal(&PropKindEntry::F32, "", ""), Ok(Value::Null));
        assert!(literal(&PropKindEntry::Enum(vec!["A".into()]), "B", "B").is_err());
    }
}
