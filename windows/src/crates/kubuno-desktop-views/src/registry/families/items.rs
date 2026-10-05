//! Component family `items` — `<Repeater>`, a list whose items are views (WPF's `ItemsControl`,
//! WinUI's `ItemsRepeater`). Compiled with the `family-items` feature (on by default through
//! `all-families`).
//!
//! ## The template
//!
//! Each row of `ItemsSource` (a [`crate::binding::Rows`] list) is shown by one instance of the
//! item template:
//!
//! * `ItemTemplate="MessageRow"` names a user control (`#[derive(UserControl)]`, its own
//!   `.kbview`): every item gets an instance of the class (its properties named like a row field
//!   are set from the row, its handlers are its methods) and a live tree of its view;
//! * else the `<Repeater>`'s one child element is the template, written in place:
//!   `<Repeater ItemsSource="{Binding Users}"><Panel …><Label Text="{Binding Name}"/></Panel></Repeater>`.
//!
//! Inside an item, a binding path reads, in order: the row's field of that name (`ItemIndex` is
//! the item's index), the user control's own property, then the view model of the page. A two-way
//! binding to a row field writes `<ItemsSource path>[<index>].<field>` to the page's view model
//! (the path `DataTable` edits use). A handler named in the template runs on the user control,
//! else on the page; [`crate::binding::current_item`] tells it which item raised it.
//!
//! ## Identity, virtualisation, cost
//!
//! Items are identified by their `ItemKey` field (the index when there is none): an item whose
//! key survives a change of the list keeps its live tree — what the user typed, the hover, the
//! focus (every element of an item gets ids and focus ids of its own, see
//! `crate::compile::with_item_scope`). Only the items in view (and a few around them) are built
//! and painted; the others are laid out from their measured or estimated extent. The list itself
//! is followed by its stamp ([`crate::binding::ListWatch`]): an unchanged list costs one
//! comparison per frame.
//!
//! ## Design time
//!
//! With no rows to show (no data at design time), the designer shows `DesignItemCount` sample
//! items whose bound fields read their own path (`{Binding Name}` shows `Name 1`, `Name 2`…). An
//! in-place template's first item is the one the designer selects and edits.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kubuno_desktop_controls::host::{self, Frame};
use kubuno_desktop_ui::range::ScrollBar;
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Size, Widget, WidgetState};

#[allow(unused_imports)] // Used by the `component!` invocation below.
use crate::registry::macros::component;
use crate::ast::{AstNode, Document, Element};
use crate::binding::{BindingSpec, ItemContext, ListWatch, PropSource, Row, Rows, Value, ViewModel};
use crate::component::Component;
use crate::events::{ChangeSource, ItemEventArgs, SelectionChangedEventArgs};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::props::{BuildCx, BuildError, Props};
use crate::registry::{ComponentMeta, LayoutKind};

component! {
    mod_name: repeater,
    name: "Repeater",
    // Note: A list of views (`ItemsControl`): see `registry::families::items`' module doc.
    doc: "A list whose items are views: each row of ItemsSource is shown by the ItemTemplate user control, or by the element written inside the Repeater. Only the items in view are built.",
    ctor: kubuno_desktop_ui::range::ScrollBar::vertical(),
    children: ChildrenModel::SingleWidget,
    default_event: "OnItemClick",
    props: [
        PropertyMeta::new("ItemsSource", PropKind::String, "", "The rows shown, one item each: a binding to a list.").category("Data").bindable().editor("list"),
        PropertyMeta::new("ItemTemplate", PropKind::String, "", "The user control that shows one item; empty to use the element written inside the Repeater.").category("Data").editor("class:UserControl"),
        PropertyMeta::new("ItemKey", PropKind::String, "", "The row field that identifies an item across changes of the list (an id); empty for the position.").category("Data"),
        PropertyMeta::new("Orientation", PropKind::Enum(&["Vertical", "Horizontal"]), "Vertical", "Whether the items follow each other downwards or to the right.").category("Layout"),
        PropertyMeta::new("Wrap", PropKind::Bool, "false", "Wraps the items onto several lines (a grid of cards) instead of one column or row.").category("Layout"),
        PropertyMeta::new("Spacing", PropKind::F32, "0", "The space between two items, in DIP.").category("Layout"),
        PropertyMeta::new("ItemWidth", PropKind::F32, "0", "The width of an item, in DIP; 0 for the full width (one column) or the template's design width (wrapped).").category("Layout"),
        PropertyMeta::new("ItemHeight", PropKind::F32, "0", "The height of an item, in DIP; 0 to measure each item.").category("Layout"),
        PropertyMeta::new("ItemHeightField", PropKind::String, "", "The row field holding each item's height, in DIP, for items of their own heights; empty to measure each item.").category("Layout"),
        PropertyMeta::new("SelectionMode", PropKind::Enum(&["None", "Single"]), "None", "Whether clicking an item selects it (and shows it selected).").category("Behavior"),
        PropertyMeta::new("SelectedIndex", PropKind::F32, "-1", "The index of the selected item, -1 for none.").category("Behavior").bindable(),
        PropertyMeta::new("EmptyText", PropKind::String, "", "The text shown when the list has no item.").category("Appearance").localizable(),
        PropertyMeta::new("DesignItemCount", PropKind::F32, "3", "How many sample items the designer shows when the list has no rows at design time.").category("Design").design_time(),
    ],
    events: [
        EventMeta::new("OnItemClick", "Occurs when an item is clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
        EventMeta::new("OnSelectionChanged", "Occurs when the selected item changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
    ],
    smoke: |b| { b.with_expanded(false).with_opacity(1.0) },
    build: |props, cx| {
        crate::registry::families::items::build_repeater(props, cx)
    },
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[repeater::META];

/// A literal attribute, or a build error for a binding (structure is read once).
fn literal(props: &Props<'_>, name: &str, default: &str) -> Result<String, BuildError> {
    match props.str(name, default)? {
        PropSource::Literal(s) => Ok(s),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), props.element().name_range())),
    }
}

/// Where the items' views come from.
enum Template {
    /// The `<Repeater>`'s own child element.
    Inline(Element),
    /// A user control: its name, and its view's root element (parsed and validated once) or why
    /// it cannot be shown.
    Class { name: String, root: Result<Element, String> },
    /// Nothing to show an item with.
    Missing,
}

impl Template {
    /// The element each item is built from.
    fn root(&self) -> Result<&Element, String> {
        match self {
            Template::Inline(e) => Ok(e),
            Template::Class { root, .. } => root.as_ref().map_err(Clone::clone),
            Template::Missing => Err("set ItemTemplate, or write the item's element inside the Repeater".to_string()),
        }
    }

    /// The template's design size (`DesignWidth`/`DesignHeight` of a user control's view, the
    /// `Width`/`Height` of an in-place element).
    fn design_size(&self) -> (Option<f32>, Option<f32>) {
        let number = |e: &Element, name: &str| e.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0);
        match self {
            Template::Inline(e) => (number(e, "Width"), number(e, "Height")),
            Template::Class { root: Ok(e), .. } => (number(e, "DesignWidth"), number(e, "DesignHeight")),
            _ => (None, None),
        }
    }

    /// The handler a user control's own view names for `Load` (`<UserControl … OnLoad="…">`).
    fn load_handler(&self) -> Option<String> {
        match self {
            Template::Class { root: Ok(e), .. } => e.attribute("OnLoad").and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
            _ => None,
        }
    }

    /// The binding paths the template reads (the fields of a design-time sample row).
    fn bound_paths(&self) -> Vec<String> {
        let Ok(root) = self.root() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for e in root.syntax().descendants().filter_map(Element::cast) {
            for a in e.attributes() {
                let Some(v) = a.value() else { continue };
                if !crate::binding::is_binding_expr(&v) {
                    continue;
                }
                if let Some(spec) = crate::binding::parse_binding(&v) {
                    // A `{Res}` reads its argument bindings' paths (WV-6).
                    let args = spec.res_args.iter().filter_map(|a| match &a.value {
                        crate::binding::ResArgSource::Binding(b) => Some(b.path.clone()),
                        crate::binding::ResArgSource::Literal(_) => None,
                    });
                    let own = (crate::resources::reference(&spec).is_none()).then(|| spec.path.clone());
                    for path in own.into_iter().chain(args) {
                        if !path.is_empty() && !out.contains(&path) {
                            out.push(path);
                        }
                    }
                }
            }
        }
        out
    }
}

/// Reads `<Repeater>`.
pub(crate) fn build_repeater(props: &Props<'_>, cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let class = literal(props, "ItemTemplate", "")?;
    let child = props.element().children().next();
    let template = if !class.trim().is_empty() {
        let name = class.trim().to_string();
        let root = class_template(&name);
        Template::Class { name, root }
    } else if let Some(child) = child {
        Template::Inline(child)
    } else {
        Template::Missing
    };
    let items_source = props.str("ItemsSource", "")?.binding().cloned();
    let design_count = match props.f32("DesignItemCount", 3.0)? {
        PropSource::Literal(v) => v.clamp(0.0, 50.0).round() as usize,
        PropSource::Bound { .. } => 3,
    };
    Ok(Box::new(RepeaterNode {
        items_source,
        template,
        base_dir: cx.base_dir.clone(),
        item_key: literal(props, "ItemKey", "")?,
        horizontal: literal(props, "Orientation", "Vertical")? == "Horizontal",
        wrap: props.bool("Wrap", false)?,
        spacing: props.f32("Spacing", 0.0)?,
        item_width: props.f32("ItemWidth", 0.0)?,
        item_height: props.f32("ItemHeight", 0.0)?,
        height_field: props.element().attribute("ItemHeightField").and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
        selectable: literal(props, "SelectionMode", "None")? == "Single",
        selected_index: props.f32("SelectedIndex", -1.0)?,
        empty_text: props.str("EmptyText", "")?,
        design_count,
        focus_id: props.focus_id(),
        on_item_click: props.event("OnItemClick"),
        on_selection_changed: props.event("OnSelectionChanged"),
        watch: ListWatch::default(),
        rows: Rows::new(),
        keys: Vec::new(),
        extents: Vec::new(),
        offsets: Vec::new(),
        offsets_dirty: true,
        measured: HashMap::new(),
        realized: HashMap::new(),
        frame_no: 0,
        scroll: 0.0,
        drag: None,
        pressed: None,
        selected: None,
        // `d:ItemsSource="design/drives.json"` (or inline JSON): the rows the designer shows.
        design_rows: None,
        design_source: design_source(props, cx.base_dir.as_deref()),
        design_scratch: crate::design::LayoutMap::new(),
        menus: None,
        build_error: None,
    }))
}

/// Where `d:ItemsSource` (XAML's `d:DesignData`) is: the attribute's value and the view's folder. Only
/// read when the designer shows sample rows ([`design_items`]): a running application never opens it.
pub(crate) fn design_source(props: &Props<'_>, base_dir: Option<&std::path::Path>) -> Option<(String, Option<std::path::PathBuf>)> {
    let value = props.element().attribute("d:ItemsSource").and_then(|a| a.value())?;
    Some((value.trim().to_string(), base_dir.map(std::path::Path::to_path_buf)))
}

/// The design-time rows of `d:ItemsSource`: a JSON array of objects, inline or in a file relative to
/// the view. `None` when it cannot be read (the generated samples then).
pub(crate) fn design_items(source: &(String, Option<std::path::PathBuf>)) -> Option<Rows> {
    let (value, base_dir) = source;
    let value = value.trim();
    let base_dir = base_dir.as_deref();
    let json = if value.starts_with('[') {
        value.to_string()
    } else {
        let path = base_dir.map(|d| d.join(value)).unwrap_or_else(|| std::path::PathBuf::from(value));
        match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("d:ItemsSource: cannot read `{}`: {e}", path.display());
                return None;
            }
        }
    };
    design_rows_from_json(&json)
}

/// The rows of a `d:ItemsSource` JSON array of objects.
fn design_rows_from_json(json: &str) -> Option<Rows> {
    // Editors (Visual Studio, PowerShell) often save UTF-8 with a byte order mark, which JSON parsers reject.
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("d:ItemsSource is not a JSON array of objects: {e}");
            return None;
        }
    };
    json_rows(&parsed)
}

/// A JSON array of objects as rows; a field holding such an array is a nested list (the rows of a Repeater
/// inside the item: `"Settings": [{ "Label": "…" }]`).
fn json_rows(parsed: &serde_json::Value) -> Option<Rows> {
    let rows: Vec<Row> = parsed
        .as_array()?
        .iter()
        .filter_map(|item| item.as_object())
        .map(|fields| {
            fields.iter().fold(Row::new(), |row, (name, v)| {
                let value = match v {
                    serde_json::Value::Bool(b) => Value::Bool(*b),
                    serde_json::Value::Number(n) => Value::F32(n.as_f64().unwrap_or_default() as f32),
                    serde_json::Value::String(s) => Value::Str(s.clone()),
                    serde_json::Value::Array(_) => json_rows(v).map(Value::List).unwrap_or_else(|| Value::Str(v.to_string())),
                    other => Value::Str(other.to_string()),
                };
                row.with(name.clone(), value)
            })
        })
        .collect();
    Some(Rows::from_vec(rows))
}

/// A design-time sample of the row field `path` for item `n` (no `d:ItemsSource`): by the kind of the user
/// control's property of that name when there is one, else by the field's name — initials are two letters, a count
/// a small number, a flag alternates, a colour, a time, a date, a picture (none: the control's placeholder), a
/// person's name; anything else `Path n`.
pub(crate) fn design_sample(path: &str, kind: Option<&crate::registry::PropKind>, n: usize) -> Value {
    match kind {
        Some(crate::registry::PropKind::F32) => return Value::F32((n + 1) as f32),
        Some(crate::registry::PropKind::Bool) => return Value::Bool(n.is_multiple_of(2)),
        Some(crate::registry::PropKind::Enum(variants)) => return Value::Str(variants.first().copied().unwrap_or_default().to_string()),
        _ => {}
    }
    let lower = path.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    const NAMES: [&str; 5] = ["Alice Martin", "Bruno Diaz", "Chloé Petit", "David Moreau", "Emma Laurent"];
    const INITIALS: [&str; 5] = ["AM", "BD", "CP", "DM", "EL"];
    const COLORS: [&str; 5] = ["#3B82F6", "#10B981", "#F59E0B", "#EF4444", "#8B5CF6"];
    if has(&["initial"]) {
        Value::Str(INITIALS[n % INITIALS.len()].to_string())
    } else if has(&["image", "avatar", "photo", "picture", "thumbnail", "url"]) {
        Value::Str(String::new())
    } else if lower.starts_with("is") || lower.starts_with("has") || lower.starts_with("show") || lower.starts_with("can") || has(&["enabled", "visible", "selected", "checked", "online", "pinned", "muted", "read"]) && !has(&["unread"]) {
        Value::Bool(n.is_multiple_of(2))
    } else if has(&["count", "unread", "number", "total", "amount", "quantity", "qty", "size", "bytes", "percent", "progress", "index", "rank", "age"]) {
        Value::F32((n + 1) as f32)
    } else if has(&["colour", "color"]) {
        Value::Str(COLORS[n % COLORS.len()].to_string())
    } else if has(&["time"]) {
        Value::Str(format!("14:{:02}", (32 + 7 * n) % 60))
    } else if has(&["date", "day"]) {
        let d = crate::clock::today();
        Value::Str(format!("{:04}-{:02}-{:02}", d.year, d.month, d.day))
    } else if has(&["icon"]) {
        Value::Str("file".to_string())
    } else if has(&["name", "author", "sender", "user", "contact", "title"]) {
        Value::Str(NAMES[n % NAMES.len()].to_string())
    } else {
        Value::Str(format!("{path} {}", n + 1))
    }
}

/// The root element of user control `name`'s view, validated.
fn class_template(name: &str) -> Result<Element, String> {
    let info = crate::registry::project::project_info(name).ok_or_else(|| format!("`{name}` is not a user control of this program (build the project)"))?;
    if info.kind != crate::registry::project::ClassKind::UserControl {
        return Err(format!("`{name}` is not a user control"));
    }
    let view = info.view.ok_or_else(|| format!("the view of `{name}` is not compiled in (build the project)"))?;
    let parse = crate::syntax::parse(view);
    let mut diagnostics = parse.diagnostics.clone();
    diagnostics.extend(crate::validate::validate(&parse, crate::registry::all()));
    if let Some(d) = diagnostics.first() {
        return Err(format!("the view of `{name}` does not compile (line {}: {})", d.line, d.message));
    }
    Document::cast(parse.syntax()).and_then(|d| d.root_element()).ok_or_else(|| format!("the view of `{name}` has no root element"))
}

/// One built item.
struct Item {
    root: Box<dyn ViewNode>,
    /// The user control's instance (an `ItemTemplate` class).
    instance: Option<Rc<RefCell<dyn Component>>>,
    /// The row last applied to the instance's properties.
    applied: Option<Row>,
    loaded: bool,
    /// The last frame it was painted in.
    used: u64,
}

/// How many built items out of view are kept (scrolling back shows them without a rebuild).
const KEEP_OUT_OF_VIEW: usize = 32;
/// The extent of an item nothing is known about yet.
const DEFAULT_EXTENT: f32 = 40.0;

/// `<Repeater>`'s live node.
pub struct RepeaterNode {
    items_source: Option<BindingSpec>,
    template: Template,
    base_dir: Option<std::path::PathBuf>,
    item_key: String,
    horizontal: bool,
    wrap: PropSource<bool>,
    spacing: PropSource<f32>,
    item_width: PropSource<f32>,
    item_height: PropSource<f32>,
    /// `ItemHeightField`: the row field holding each item's height (items of their own heights:
    /// a card per category, as tall as its rows). Without it, an item is measured.
    height_field: Option<String>,
    selectable: bool,
    selected_index: PropSource<f32>,
    empty_text: PropSource<String>,
    design_count: usize,
    focus_id: Option<FocusId>,
    on_item_click: Option<String>,
    on_selection_changed: Option<String>,
    watch: ListWatch,
    /// The rows shown, their keys, and each one's extent along the flow.
    rows: Rows,
    keys: Vec<String>,
    extents: Vec<f32>,
    /// `offsets[i]`: where item `i` starts along the flow (`offsets[n]`: the end), one column.
    offsets: Vec<f32>,
    offsets_dirty: bool,
    /// Measured extents, by key (kept across list changes).
    measured: HashMap<String, f32>,
    realized: HashMap<String, Item>,
    frame_no: u64,
    /// The scroll offset along the flow.
    scroll: f32,
    /// The scroll bar's thumb being dragged: where it was grabbed, along the axis.
    drag: Option<f32>,
    /// The item under a press (its key), for `OnItemClick`.
    pressed: Option<String>,
    selected: Option<usize>,
    /// The sample rows of the designer (built once).
    design_rows: Option<Rows>,
    /// `d:ItemsSource`, read the first time the designer needs sample rows.
    design_source: Option<(String, Option<std::path::PathBuf>)>,
    design_scratch: crate::design::LayoutMap,
    /// The context menus of an `ItemTemplate` user control's own view (read once).
    menus: Option<Vec<Rc<crate::window::MenuSpec>>>,
    /// Why the items cannot be built (painted in place of the list).
    build_error: Option<String>,
}

/// The view model an item paints with: its row, then its user control, then the page.
struct ItemVm<'a> {
    row: &'a Row,
    index: usize,
    list_path: &'a str,
    instance: Option<&'a mut dyn ViewModel>,
    outer: &'a mut dyn ViewModel,
}

impl ViewModel for ItemVm<'_> {
    fn get(&self, path: &str) -> Option<Value> {
        if path == "ItemIndex" {
            return Some(Value::F32(self.index as f32));
        }
        if let Some(v) = self.row.get(path).or_else(|| path.strip_prefix("Item.").and_then(|p| self.row.get(p))) {
            return Some(v.clone());
        }
        if let Some(v) = self.instance.as_deref().and_then(|i| i.get(path)) {
            return Some(v);
        }
        self.outer.get(path)
    }

    fn set(&mut self, path: &str, value: Value) {
        let field = path.strip_prefix("Item.").unwrap_or(path);
        if self.row.get(field).is_some() {
            if !self.list_path.is_empty() {
                self.outer.set(&format!("{}[{}].{field}", self.list_path, self.index), value);
            }
            return;
        }
        if let Some(i) = self.instance.as_deref_mut() {
            if i.get(path).is_some() {
                i.set(path, value);
                return;
            }
        }
        self.outer.set(path, value);
    }

    fn dispatch_event(&mut self, handler: &str, sender: &crate::events::ElementRef<'_>, args: &mut dyn crate::events::EventArgs) -> bool {
        if let Some(i) = self.instance.as_deref_mut() {
            if i.dispatch_event(handler, sender, args) {
                return true;
            }
        }
        self.outer.dispatch_event(handler, sender, args)
    }
}

/// The scope of an item's elements in the window's router (`crate::events::router::DispatchScope`): their handlers
/// read the item's row, then its user control, then the page ([`ItemVm`]), with the item current.
struct ItemScope {
    context: ItemContext,
    list_path: String,
    instance: Option<std::rc::Weak<RefCell<dyn Component>>>,
}

impl crate::events::router::DispatchScope for ItemScope {
    fn with_dispatch(&self, d: &mut crate::events::router::Dispatch<'_>, f: &mut dyn FnMut(&mut crate::events::router::Dispatch<'_>)) {
        let cell = self.instance.as_ref().and_then(std::rc::Weak::upgrade);
        let mut guard = cell.as_ref().and_then(|c| c.try_borrow_mut().ok());
        let instance = guard.as_deref_mut().and_then(|c| c.kubuno_view_model());
        let mut item_vm = ItemVm { row: &self.context.row, index: self.context.index, list_path: &self.list_path, instance, outer: &mut *d.vm };
        let mut nested = crate::events::router::Dispatch { vm: &mut item_vm, handlers: &mut *d.handlers, events: &mut *d.events };
        crate::binding::with_current_item(self.context.clone(), || f(&mut nested));
    }
}

/// A 64-bit hash of an item key (its id scope).
fn key_hash(key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl RepeaterNode {
    /// The key of row `i`.
    fn key_of(&self, row: &Row, i: usize) -> String {
        match (!self.item_key.is_empty()).then(|| row.get(&self.item_key)).flatten() {
            Some(Value::Str(s)) => format!("k:{s}"),
            Some(Value::F32(f)) => format!("k:{f}"),
            Some(Value::Bool(b)) => format!("k:{b}"),
            _ => format!("i:{i}"),
        }
    }

    /// The design-time sample rows.
    fn design_rows(&mut self) -> Rows {
        if let Some(rows) = &self.design_rows {
            return rows.clone();
        }
        if let Some(rows) = self.design_source.take().and_then(|s| design_items(&s)) {
            self.design_rows = Some(rows.clone());
            return rows;
        }
        let paths = self.template.bound_paths();
        // A field named like a property of the user control takes a sample of that property's kind
        // (a count reads `1`, not `Likes 1`, which would not fit where a number goes).
        let own: &[crate::registry::PropertyMeta] = match &self.template {
            Template::Class { name, .. } => crate::registry::project::project_info(name).map(|i| i.own_properties).unwrap_or(&[]),
            _ => &[],
        };
        let sample = |p: &str, n: usize| -> Value { design_sample(p, own.iter().find(|m| m.name == p).map(|m| &m.kind), n) };
        let rows: Rows = (0..self.design_count).map(|n| paths.iter().fold(Row::new(), |row, p| row.with(p.clone(), sample(p, n)))).collect();
        self.design_rows = Some(rows.clone());
        rows
    }

    /// Takes `rows` as the list shown: keys, extents (measured ones kept by key).
    fn set_rows(&mut self, rows: Rows) {
        self.keys = rows.iter().enumerate().map(|(i, r)| self.key_of(r, i)).collect();
        self.rows = rows;
        self.offsets_dirty = true;
        // Built items whose key is gone are dropped (their instances disposed).
        let keys: std::collections::HashSet<&String> = self.keys.iter().collect();
        self.realized.retain(|k, _| keys.contains(k));
        self.measured.retain(|k, _| keys.contains(k));
        if let Some(sel) = self.selected {
            if sel >= self.rows.len() {
                self.selected = None;
            }
        }
    }

    /// The extent of item `i` along the flow: fixed, measured, else estimated.
    fn extent_of(&self, i: usize, fixed: f32, estimate: f32) -> f32 {
        if fixed > 0.0 {
            return fixed;
        }
        self.keys.get(i).and_then(|k| self.measured.get(k)).copied().unwrap_or(estimate)
    }

    /// The estimate of an unmeasured item: the template's design extent, else the mean of the
    /// measured ones.
    fn estimate(&self) -> f32 {
        let (w, h) = self.template.design_size();
        let design = if self.horizontal { w } else { h };
        if let Some(d) = design {
            return d;
        }
        if self.measured.is_empty() {
            return DEFAULT_EXTENT;
        }
        self.measured.values().sum::<f32>() / self.measured.len() as f32
    }

    /// Recomputes `offsets` (one column/row, no wrap).
    fn layout_offsets(&mut self, fixed: f32, spacing: f32) {
        let n = self.rows.len();
        let estimate = self.estimate();
        let extents: Vec<f32> = (0..n).map(|i| self.extent_of(i, fixed, estimate)).collect();
        let mut offsets = Vec::with_capacity(n + 1);
        let mut at = 0.0;
        for e in &extents {
            offsets.push(at);
            at += e + spacing;
        }
        offsets.push(if n > 0 { at - spacing } else { 0.0 });
        self.extents = extents;
        self.offsets = offsets;
        self.offsets_dirty = false;
    }
}

/// The geometry of one frame.
struct Flow {
    horizontal: bool,
    wrap: bool,
    /// Items per line (wrap), else 1.
    per_line: usize,
    /// An item's size across the flow.
    cross: f32,
    /// A wrapped line's extent along the flow.
    line: f32,
    spacing: f32,
    /// The whole content's extent along the flow.
    total: f32,
}

impl Flow {
    /// Item `i`'s rectangle in `bounds` at scroll `scroll`.
    fn rect(&self, node: &RepeaterNode, bounds: Rect, i: usize, scroll: f32) -> Rect {
        let (main, main_len, cross_at) = if self.wrap {
            let line = i / self.per_line.max(1);
            let pos = i % self.per_line.max(1);
            (line as f32 * (self.line + self.spacing), self.line, pos as f32 * (self.cross + self.spacing))
        } else {
            (node.offsets.get(i).copied().unwrap_or(0.0), node.extents.get(i).copied().unwrap_or(0.0), 0.0)
        };
        if self.horizontal {
            let left = bounds.left + main - scroll;
            let top = bounds.top + cross_at;
            Rect::new(left, top, left + main_len, top + self.cross)
        } else {
            let top = bounds.top + main - scroll;
            let left = bounds.left + cross_at;
            Rect::new(left, top, left + self.cross, top + main_len)
        }
    }

    /// The items at least partly inside a viewport of `viewport` along the flow, at `scroll`.
    fn visible(&self, node: &RepeaterNode, scroll: f32, viewport: f32) -> std::ops::Range<usize> {
        let n = node.rows.len();
        if n == 0 {
            return 0..0;
        }
        if self.wrap {
            let pitch = (self.line + self.spacing).max(1.0);
            let first_line = (scroll / pitch).floor().max(0.0) as usize;
            let last_line = ((scroll + viewport) / pitch).ceil().max(0.0) as usize;
            let per = self.per_line.max(1);
            return (first_line * per).min(n)..((last_line + 1) * per).min(n);
        }
        // `offsets` is sorted: the first item ending after `scroll`, the first starting after the end.
        let first = node.offsets[..n].partition_point(|&o| o <= scroll).saturating_sub(1);
        let last = node.offsets[..n].partition_point(|&o| o < scroll + viewport);
        first..last.max(first).min(n)
    }
}

impl ViewNode for RepeaterNode {
    fn measure(&self, _c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let (w, h) = self.template.design_size();
        let total = self.offsets.last().copied().unwrap_or(0.0);
        let extent = if total > 0.0 { total.min(480.0) } else { 240.0 };
        let cross = if self.horizontal { self.item_height.resolve(vm).max(h.unwrap_or(0.0)) } else { self.item_width.resolve(vm).max(w.unwrap_or(0.0)) };
        let cross = if cross > 0.0 { cross } else { 320.0 };
        if self.horizontal { Size::new(extent, cross) } else { Size::new(cross, extent) }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.frame_no += 1;
        let design = cx.design.is_some();
        let canvas: &dyn Canvas = cx.canvas;

        // The rows: the bound list when it changed; sample rows in the designer without data.
        if let Some(rows) = self.watch.changed(&*cx.vm, self.items_source.as_ref()) {
            if !(design && rows.is_empty()) {
                self.set_rows(rows);
            }
        }
        if design && self.rows.is_empty() && self.watch.current().is_none_or(|r| r.is_empty()) && self.design_count > 0 {
            let rows = self.design_rows();
            if !self.rows.same(&rows) {
                self.set_rows(rows);
            }
        }
        if let Err(why) = self.template.root() {
            crate::node::custom::paint_placeholder(cx.canvas, bounds, "Repeater", &why);
            return;
        }
        if let Some(why) = &self.build_error {
            crate::node::custom::paint_placeholder(cx.canvas, bounds, "Repeater", why);
            return;
        }

        // The geometry.
        let vm: &dyn ViewModel = &*cx.vm;
        let spacing = self.spacing.resolve(vm).max(0.0);
        let wrap = self.wrap.resolve(vm);
        let (item_w, item_h) = (self.item_width.resolve(vm).max(0.0), self.item_height.resolve(vm).max(0.0));
        let (design_w, design_h) = self.template.design_size();
        let (bw, bh) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let (viewport, cross_avail) = if self.horizontal { (bw, bh) } else { (bh, bw) };
        let (fixed_main, fixed_cross, design_main, design_cross) =
            if self.horizontal { (item_w, item_h, design_w, design_h) } else { (item_h, item_w, design_h, design_w) };
        let wanted = self.selected_index.resolve(vm);
        let wanted = (wanted >= 0.0).then(|| wanted.round() as usize).filter(|i| *i < self.rows.len());
        if self.selectable && wanted != self.selected {
            self.selected = wanted;
        }
        let n = self.rows.len();
        let flow = if wrap {
            let cross = if fixed_cross > 0.0 { fixed_cross } else { design_cross.unwrap_or(160.0) };
            let line = if fixed_main > 0.0 { fixed_main } else { design_main.or_else(|| self.measured.values().copied().reduce(f32::max)).unwrap_or(DEFAULT_EXTENT) };
            let per_line = (((cross_avail + spacing) / (cross + spacing).max(1.0)).floor() as usize).max(1);
            let lines = n.div_ceil(per_line);
            Flow { horizontal: self.horizontal, wrap: true, per_line, cross, line, spacing, total: if lines > 0 { lines as f32 * (line + spacing) - spacing } else { 0.0 } }
        } else {
            if self.offsets_dirty || self.offsets.len() != n + 1 {
                self.layout_offsets(fixed_main, spacing);
            }
            let cross = if fixed_cross > 0.0 { fixed_cross.min(cross_avail) } else { cross_avail };
            Flow { horizontal: self.horizontal, wrap: false, per_line: 1, cross, line: 0.0, spacing, total: self.offsets.last().copied().unwrap_or(0.0) }
        };

        // Scrolling: the wheel over the list, the scroll bar's thumb.
        let max_scroll = (flow.total - viewport).max(0.0);
        let frame: &Frame = cx.frame;
        let (mx, my) = frame.mouse;
        let inside = !frame.pointer_outside() && bounds.contains(mx, my);
        if !design {
            let (wx, wy) = frame.wheel_dip();
            let delta = if self.horizontal && wx != 0.0 { wx } else { wy };
            if inside && delta != 0.0 && max_scroll > 0.0 && !host::wheel_claimed() {
                let before = self.scroll;
                // The host's wheel is in the web's sign: positive moves toward the end.
                self.scroll = (self.scroll + delta).clamp(0.0, max_scroll);
                if self.scroll != before {
                    host::claim_wheel();
                }
            }
        }
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let mut bar = ScrollBar::from_content(self.horizontal, flow.total, viewport, self.scroll);
        let rail = bar.as_ref().map(|b| b.rail(&bounds));
        let over_bar = rail.is_some_and(|r| r.contains(mx, my)) && inside;
        if let (Some(b), Some(rail)) = (bar.as_mut(), rail) {
            b.expanded = over_bar || self.drag.is_some();
            if !design && frame.mouse_down {
                let along = if self.horizontal { mx } else { my };
                match self.drag {
                    Some(grab) => {
                        let v = b.value_at_thumb_start(rail, along - grab);
                        self.scroll = (v as f32).clamp(0.0, max_scroll);
                    }
                    None if over_bar && self.pressed.is_none() => {
                        let thumb = b.thumb_rect(rail);
                        if thumb.contains(mx, my) {
                            self.drag = Some(along - if self.horizontal { thumb.left } else { thumb.top });
                        } else if let Some(part) = b.part_at(rail, mx, my) {
                            if b.apply_part(part) {
                                self.scroll = (b.value() as f32).clamp(0.0, max_scroll);
                            }
                            self.drag = Some(f32::MAX);
                        }
                    }
                    None => {}
                }
                if self.drag == Some(f32::MAX) {
                    // A track click pages once per press.
                } else {
                    b.set_content(flow.total, viewport, self.scroll);
                }
            } else {
                self.drag = None;
            }
        }

        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let _ = focus_state;

        // The items in view (and one around).
        let visible = flow.visible(self, self.scroll, viewport);
        let range = visible.start.saturating_sub(1)..(visible.end + 1).min(n);
        let list_path = self.items_source.as_ref().map(|s| s.path.clone()).unwrap_or_default();
        let theme = canvas.theme().clone();
        // The items see the pointer only inside the list, and not over its scroll bar.
        let item_frame = if inside && !over_bar && self.drag.is_none() {
            *frame
        } else {
            Frame { mouse: (host::POINTER_AWAY, host::POINTER_AWAY), mouse_down: false, right_down: false, middle_down: false, wheel: (0.0, 0.0), ..*frame }
        };
        let mut hot: Option<usize> = None;
        // A control inside the hot item answered the pointer this frame (a button, a check box):
        // the item does not also report the click (WPF's `e.Handled`).
        let mut handled_inside = false;
        let mut remeasured = false;
        canvas.push_clip(&bounds);
        for i in range.clone() {
            let rect = flow.rect(self, bounds, i, self.scroll);
            if rect.bottom < bounds.top || rect.top > bounds.bottom || rect.right < bounds.left || rect.left > bounds.right {
                continue;
            }
            let key = self.keys[i].clone();
            let row = self.rows[i].clone();
            if !self.realized.contains_key(&key) {
                // In the designer, the first item of an in-place template is the one it edits.
                let record = design && i == 0 && matches!(self.template, Template::Inline(_));
                match self.build_item(&key, record) {
                    Ok(item) => {
                        self.realized.insert(key.clone(), item);
                    }
                    Err(e) => {
                        self.build_error = Some(e);
                        break;
                    }
                }
            }
            let item_hot = !design && inside && !over_bar && rect.contains(mx, my);
            if item_hot {
                hot = Some(i);
            }
            // The selection and the hover, under the item.
            if self.selectable {
                let pill = rect;
                if self.selected == Some(i) {
                    canvas.fill_rounded(&pill, kubuno_desktop_ui::metrics::radius::SM, &theme.list_selected);
                } else if item_hot {
                    canvas.fill_rounded(&pill, kubuno_desktop_ui::metrics::radius::SM, &theme.row_hover);
                }
            }
            let class = match &self.template {
                Template::Class { name, .. } => crate::registry::project::project_info(name),
                _ => None,
            };
            let Some(item) = self.realized.get_mut(&key) else { continue };
            item.used = self.frame_no;
            let record = design && i == 0 && matches!(self.template, Template::Inline(_));
            let mut guard = item.instance.as_ref().and_then(|c| c.try_borrow_mut().ok());
            if let Some(component) = guard.as_deref_mut() {
                // The row's fields named like the user control's properties are set on it.
                if item.applied.as_ref() != Some(&row) {
                    if let Some(info) = class {
                        for p in info.own_properties {
                            if let Some(v) = row.get(p.name) {
                                component.kubuno_set_property(p.name, v);
                            }
                        }
                    }
                    item.applied = Some(row.clone());
                }
                // Load, once (in the designer too, with `design_mode()` true: a user control's sample data), as
                // a user control placed on a form does (`crate::node::custom::load_user_control`).
                if !item.loaded {
                    item.loaded = true;
                    let handler = self.template.load_handler();
                    if let Some(control) = component.as_control_mut() {
                        if let Err(why) = crate::node::custom::load_user_control(control, handler.as_deref(), rect, design) {
                            tracing::warn!("a Repeater item: {why}");
                        }
                    }
                }
            }
            let instance_vm = guard.as_deref_mut().and_then(|c| c.kubuno_view_model());
            let mut item_vm = ItemVm { row: &row, index: i, list_path: &list_path, instance: instance_vm, outer: &mut *cx.vm };
            // Variable extents: an item is measured at the width it gets.
            if fixed_main <= 0.0 && !flow.wrap {
                let size = if self.horizontal { item.root.measure(canvas, &item_vm) } else { item.root.measure_for_width(canvas, &item_vm, flow.cross) };
                let m = if self.horizontal { size.width } else { size.height };
                let m = self.height_field.as_deref().and_then(|f| row.text(f).trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(m);
                if m > 0.0 && self.measured.get(&key).is_none_or(|old| (old - m).abs() > 0.5) {
                    self.measured.insert(key.clone(), m);
                    remeasured = true;
                }
            }
            if record {
                self.design_scratch.clear();
            }
            let scratch = &mut self.design_scratch;
            let design_map = if record { cx.design.as_deref_mut() } else if design { Some(scratch) } else { None };
            // The item's elements are routed like the page's, their handlers run on the item (its row, its user
            // control, then the page): a control inside an item gets its mouse, wheel and key events.
            let item_scope: Option<Rc<dyn crate::events::router::DispatchScope>> = match cx.router.as_deref_mut() {
                Some(router) => {
                    let context = ItemContext { index: i, key: key.clone(), row: row.clone() };
                    let instance = item.instance.as_ref().map(Rc::downgrade);
                    let scope: Rc<dyn crate::events::router::DispatchScope> = Rc::new(ItemScope { context, list_path: list_path.clone(), instance });
                    router.push_scope(scope.clone());
                    Some(scope)
                }
                None => None,
            };
            let scoped = item_scope.is_some();
            let menus_before = cx.services.as_deref().map_or(0, |s| s.context_menus.len());
            let mut inner = PaintCx {
                canvas: cx.canvas,
                frame: &item_frame,
                vm: &mut item_vm,
                focus: &mut *cx.focus,
                handlers: &mut *cx.handlers,
                events: &mut *cx.events,
                design: design_map,
                router: cx.router.as_deref_mut(),
                sender: cx.sender.clone(),
                control: None,
                services: cx.services.as_deref_mut(),
                activate: false,
            };
            let context = ItemContext { index: i, key: key.clone(), row: row.clone() };
            let root = &mut item.root;
            let before = crate::node::pointer_handled();
            let start = inner.router.as_deref().map(|r| r.registered_count());
            // The item clips what it shows to its own box (its user control's view, a template larger than the
            // item), as a list's item control does in Windows Forms (`crate::clip`).
            let item_clip = crate::clip::Children::push(rect);
            crate::binding::with_current_item(context, || root.paint(&mut inner, rect));
            drop(item_clip);
            if let Some(router) = cx.router.as_deref_mut() {
                if scoped {
                    router.pop_scope();
                }
                // An `ItemTemplate` user control: its view's root is the item's instance (its overrides get the pointer).
                if let (Some(start), Some(instance)) = (start, item.instance.as_ref()) {
                    router.attach_view_root_control(start, instance);
                }
            }
            // The context menus of an `ItemTemplate` user control's own view: offered per item, run on the item.
            if let (Some(scope), Some(services)) = (item_scope.as_ref(), cx.services.as_deref_mut()) {
                if self.menus.is_none() {
                    let menus = match &self.template {
                        Template::Class { root: Ok(r), .. } => crate::window::read_menus(r).into_iter().map(Rc::new).collect(),
                        _ => Vec::new(),
                    };
                    self.menus = Some(menus);
                }
                let menus = self.menus.as_deref().unwrap_or_default();
                if !menus.is_empty() {
                    let tag = format!("{:p}/{key}", &self.template as *const Template);
                    for entry in services.context_menus.iter_mut().skip(menus_before) {
                        if menus.iter().any(|m| m.name == entry.2) {
                            entry.2 = format!("{}@{tag}", entry.2);
                        }
                    }
                    for menu in menus {
                        services.local_menus.push(crate::common::LocalMenu { key: format!("{}@{tag}", menu.name), name: menu.name.clone(), spec: menu.clone(), scope: Some(scope.clone()) });
                    }
                }
            }
            if item_hot && crate::node::pointer_handled() != before {
                handled_inside = true;
            }
            // A card (a wrapped item) paints its own surface over the selection fill: its ring shows it.
            if self.selectable && flow.wrap && self.selected == Some(i) {
                canvas.stroke_rounded_w(&rect, kubuno_desktop_ui::metrics::radius::XL, &theme.accent, 2.0);
            }
        }
        canvas.pop_clip();
        if remeasured {
            self.offsets_dirty = true;
            host::request_repaint_after(0);
        }

        // Nothing to show.
        if n == 0 {
            let text = self.empty_text.resolve(&*cx.vm);
            if !text.is_empty() {
                canvas.text_ellipsis_center(&text, &bounds, &canvas.formats().body, &theme.text_secondary);
            }
        }
        if let (Some(b), Some(rail)) = (bar.as_ref(), rail) {
            b.paint(canvas, rail, WidgetState::REST.hot(over_bar));
        }

        // Clicks on an item: `OnItemClick`, and the selection.
        if design {
            return;
        }
        if frame.mouse_down && self.pressed.is_none() && self.drag.is_none() {
            if handled_inside {
                self.pressed = Some(String::new());
            } else if let Some(i) = hot {
                self.pressed = Some(self.keys[i].clone());
            } else if inside {
                self.pressed = Some(String::new());
            }
        }
        if !frame.mouse_down {
            if let (Some(pressed), Some(i)) = (self.pressed.take(), hot) {
                if !pressed.is_empty() && !handled_inside && self.keys.get(i) == Some(&pressed) {
                    self.item_clicked(cx, i);
                }
            }
            self.pressed = None;
        }

        // Built items long out of view are dropped.
        if self.realized.len() > range.len() + KEEP_OUT_OF_VIEW {
            let mut by_age: Vec<(u64, String)> = self.realized.iter().map(|(k, it)| (it.used, k.clone())).collect();
            by_age.sort_unstable();
            let excess = self.realized.len() - (range.len() + KEEP_OUT_OF_VIEW);
            for (_, k) in by_age.into_iter().take(excess) {
                self.realized.remove(&k);
            }
        }
    }
}

impl RepeaterNode {
    /// Builds the item of `key` from the template (and its user control's instance).
    fn build_item(&self, key: &str, record: bool) -> Result<Item, String> {
        let root = self.template.root()?.clone();
        let mut cx = BuildCx::new();
        cx.base_dir = self.base_dir.clone();
        let scope = if record { None } else { Some(key_hash(key)) };
        let root = crate::compile::with_item_scope(scope, || crate::compile::build_node(&root, &mut cx, LayoutKind::None)).map_err(|e| e.message)?;
        let root = crate::compile::with_top_layer(root, &mut cx);
        let instance = match &self.template {
            Template::Class { name, .. } => crate::controls::class_of(name).and_then(|c| (c.create)()),
            _ => None,
        };
        Ok(Item { root, instance, applied: None, loaded: false, used: self.frame_no })
    }

    /// Item `i` was clicked: `OnItemClick`, then the selection.
    fn item_clicked(&mut self, cx: &mut PaintCx<'_>, i: usize) {
        let context = ItemContext { index: i, key: self.keys[i].clone(), row: self.rows[i].clone() };
        let focus_id = self.focus_id;
        let handler = self.on_item_click.clone();
        crate::binding::with_current_item(context, || {
            cx.fire("OnItemClick", focus_id, handler.as_deref(), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
        });
        if self.selectable && self.selected != Some(i) {
            let old = self.selected;
            self.selected = Some(i);
            if let Some(spec) = self.selected_index.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::F32(i as f32));
                }
            }
            let mut args = SelectionChangedEventArgs::new(old, Some(i), ChangeSource::User);
            cx.fire("OnSelectionChanged", focus_id, self.on_selection_changed.as_deref(), ViewEventKind::Changed(i.to_string()), &mut args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::MapViewModel;
    use crate::runtime::Runtime;
    use kubuno_desktop_controls::host::Modifiers;
    use kubuno_desktop_ui::graphics::testing::RecordingCanvas;

    fn frame(mouse: Option<(f32, f32)>, down: bool, wheel: f32) -> Frame {
        let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
        Frame {
            size: (300.0, 200.0),
            mouse: (x, y),
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 300.0, 200.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, wheel),
            click_count: 1,
            window_focused: true,
        }
    }

    fn rows(n: usize) -> Rows {
        (0..n).map(|i| Row::new().with("Id", Value::Str(format!("id{i}"))).with("Name", Value::Str(format!("Name {i}")))).collect()
    }

    const VIEW: &str = r#"<Panel DesignWidth="300" DesignHeight="200">
  <Repeater x:Name="list" ItemsSource="{Binding Items}" ItemKey="Id" ItemHeight="20" SelectionMode="Single" SelectedIndex="{Binding Selected, Mode=TwoWay}" OnItemClick="clicked" X="0" Y="0" Width="300" Height="200">
    <Label Text="{Binding Name}"/>
  </Repeater>
</Panel>"#;

    fn run(rt: &mut Runtime, vm: &mut MapViewModel, f: Frame) -> RecordingCanvas {
        let canvas = RecordingCanvas::new();
        let mut handlers = crate::binding::HandlerTable::new();
        rt.frame(&canvas, &f, vm, &mut handlers, Rect::new(0.0, 0.0, 300.0, 200.0));
        canvas
    }

    fn shown(c: &RecordingCanvas) -> Vec<String> {
        c.calls().into_iter().filter(|s| s.starts_with("text") && s.contains("Name ")).collect()
    }

    #[test]
    fn only_the_items_in_view_are_built_and_painted() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Items", Value::List(rows(10_000)));
        let c = run(&mut rt, &mut vm, frame(None, false, 0.0));
        let texts = shown(&c);
        // 200 DIP of 20 DIP items: ten in view, one more around.
        assert!(texts.len() >= 10 && texts.len() <= 12, "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Name 0")));
        assert!(!texts.iter().any(|t| t.contains("Name 50")));
    }

    #[test]
    fn an_item_takes_the_height_its_row_names() {
        const TALL: &str = r#"<Panel DesignWidth="300" DesignHeight="200">
  <Repeater x:Name="list" ItemsSource="{Binding Items}" ItemKey="Id" ItemHeightField="H" X="0" Y="0" Width="300" Height="200">
    <Label Text="{Binding Name}" Height="20"/>
  </Repeater>
</Panel>"#;
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(TALL), "{:?}", rt.diagnostics());
        let items = crate::binding::Rows::from(
            (0..3).map(|i| crate::binding::Row::new().with("Id", Value::F32(i as f32)).with("Name", Value::Str(format!("Name {i}"))).with("H", Value::F32(if i == 0 { 150.0 } else { 20.0 }))).collect::<Vec<_>>(),
        );
        let mut vm = MapViewModel::new().with("Items", Value::List(items));
        run(&mut rt, &mut vm, frame(None, false, 0.0));
        let c = run(&mut rt, &mut vm, frame(None, false, 0.0));
        let texts = shown(&c);
        // The first item is 150 tall: the second starts at 150, the third at 170, both in view.
        assert!(texts.iter().any(|t| t.contains("Name 2")), "{texts:?}");
        let top = |name: &str| texts.iter().find(|t| t.contains(name)).and_then(|t| t.split(' ').nth(2)).and_then(|r| r.split(',').nth(1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(-1.0);
        assert!(top("Name 1") >= 150.0, "{texts:?}");
    }

    #[test]
    fn the_wheel_scrolls_and_a_click_selects_the_item_under_the_pointer() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Items", Value::List(rows(100)));
        run(&mut rt, &mut vm, frame(None, false, 0.0));
        // One notch down.
        run(&mut rt, &mut vm, frame(Some((100.0, 100.0)), false, 1.0));
        let c = run(&mut rt, &mut vm, frame(None, false, 0.0));
        assert!(!shown(&c).iter().any(|t| t.contains("Name 0\"") || t.ends_with("Name 0")), "scrolled away from the first item");
        // A click on the first row in view selects it (two-way SelectedIndex).
        run(&mut rt, &mut vm, frame(Some((100.0, 5.0)), true, 0.0));
        run(&mut rt, &mut vm, frame(Some((100.0, 5.0)), false, 0.0));
        match vm.get("Selected") {
            Some(Value::F32(i)) => assert!(i > 0.0, "an item below the first: {i}"),
            other => panic!("no selection written back: {other:?}"),
        }
    }

    #[test]
    fn a_click_handled_by_a_button_inside_an_item_is_not_an_item_click() {
        const WITH_BUTTON: &str = r#"<Panel DesignWidth="300" DesignHeight="200">
  <Repeater x:Name="list" ItemsSource="{Binding Items}" ItemKey="Id" ItemHeight="30" SelectionMode="Single" OnItemClick="clicked" X="0" Y="0" Width="300" Height="200">
    <Stack Direction="LeftToRight" Gap="0"><Button Text="Like" OnClick="like" Width="80" Height="30"/><Label Text="{Binding Name}"/></Stack>
  </Repeater>
</Panel>"#;
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(WITH_BUTTON), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Items", Value::List(rows(3)));
        let mut handlers = crate::binding::HandlerTable::new();
        let mut step = |rt: &mut Runtime, vm: &mut MapViewModel, f: Frame| {
            let canvas = RecordingCanvas::new();
            rt.frame(&canvas, &f, vm, &mut handlers, Rect::new(0.0, 0.0, 300.0, 200.0))
        };
        let names = |events: &[crate::node::ViewEvent]| events.iter().filter_map(|e| e.handler.clone()).collect::<Vec<_>>();
        step(&mut rt, &mut vm, frame(None, false, 0.0));
        // On the button: its Click only.
        step(&mut rt, &mut vm, frame(Some((40.0, 15.0)), false, 0.0));
        let mut seen = names(&step(&mut rt, &mut vm, frame(Some((40.0, 15.0)), true, 0.0)));
        seen.extend(names(&step(&mut rt, &mut vm, frame(Some((40.0, 15.0)), false, 0.0))));
        assert!(seen.iter().any(|h| h == "like"), "{seen:?}");
        assert!(!seen.iter().any(|h| h == "clicked"), "the item did not also report it: {seen:?}");
        // Beside the button: the item's click.
        step(&mut rt, &mut vm, frame(Some((200.0, 45.0)), false, 0.0));
        let mut seen = names(&step(&mut rt, &mut vm, frame(Some((200.0, 45.0)), true, 0.0)));
        seen.extend(names(&step(&mut rt, &mut vm, frame(Some((200.0, 45.0)), false, 0.0))));
        assert!(seen.iter().any(|h| h == "clicked"), "{seen:?}");
    }

    #[test]
    fn an_item_keeps_its_live_tree_when_the_list_changes_around_it() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Items", Value::List(rows(5)));
        run(&mut rt, &mut vm, frame(None, false, 0.0));
        // A row inserted at the top: every other item keeps its key, so its node.
        let mut more = rows(5);
        more.make_mut().insert(0, Row::new().with("Id", Value::Str("new".into())).with("Name", Value::Str("Name new".into())));
        vm.set("Items", Value::List(more));
        let c = run(&mut rt, &mut vm, frame(None, false, 0.0));
        assert!(shown(&c).iter().any(|t| t.contains("Name new")));
        assert_eq!(key_hash("k:id1"), key_hash("k:id1"));
    }

    #[test]
    fn the_designer_shows_sample_items_read_from_the_template() {
        let p = crate::syntax::parse(VIEW);
        let doc = Document::cast(p.syntax()).expect("document");
        let el = doc.root_element().and_then(|r| r.children().next()).expect("repeater");
        let meta = crate::registry::lookup("Repeater").expect("registered");
        let props = Props::new(&el, meta);
        let mut cx = BuildCx::new();
        let node = build_repeater(&props, &mut cx).expect("builds");
        let _ = node;
        let t = Template::Inline(el.children().next().expect("template"));
        assert_eq!(t.bound_paths(), vec!["Name".to_string()]);
    }

    #[test]
    fn a_repeater_without_template_says_so() {
        let t = Template::Missing;
        assert!(t.root().is_err());
        let bad = Template::Class { name: "Nope".into(), root: class_template("Nope") };
        assert!(bad.root().err().is_some_and(|e| e.contains("Nope")));
    }
}

#[cfg(test)]
mod design_sample_tests {
    use super::{design_rows_from_json, design_sample};
    use crate::binding::Value;

    /// Found live: a `drives.design.json` saved with a byte order mark showed the generic samples instead.
    #[test]
    fn design_data_files_may_start_with_a_byte_order_mark() {
        let rows = design_rows_from_json("\u{feff}[{\"Label\":\"Disque local (C:)\",\"UsedBytes\":3}]").expect("rows");
        assert_eq!(rows.len(), 1);
        assert!(design_rows_from_json("not json").is_none());
    }

    /// Found by the shell's storage cards: a nested array (an inner Repeater's rows) arrived as text, so the inner
    /// Repeater showed generated samples instead of the file's.
    #[test]
    fn a_nested_array_is_a_nested_list() {
        let rows = design_rows_from_json(r#"[{"Title":"Général","Settings":[{"Label":"Langue"},{"Label":"Nom"}]}]"#).expect("rows");
        match rows.first().and_then(|r| r.get("Settings")) {
            Some(Value::List(inner)) => assert_eq!(inner.len(), 2),
            other => panic!("expected a nested list, got {other:?}"),
        }
    }

    /// Found by the chat migration: a sample `Initials` read « INITIA… » and an unread count « U… ».
    #[test]
    fn samples_follow_the_field_names_and_kinds() {
        assert_eq!(design_sample("Initials", None, 0), Value::Str("AM".into()));
        assert_eq!(design_sample("UnreadCount", None, 1), Value::F32(2.0));
        assert_eq!(design_sample("Unread", None, 0), Value::F32(1.0));
        assert_eq!(design_sample("IsOnline", None, 1), Value::Bool(false));
        assert_eq!(design_sample("AccentColor", None, 0), Value::Str("#3B82F6".into()));
        assert_eq!(design_sample("Time", None, 0), Value::Str("14:32".into()));
        assert_eq!(design_sample("AvatarImage", None, 0), Value::Str(String::new()));
        assert_eq!(design_sample("Name", None, 1), Value::Str("Bruno Diaz".into()));
        assert_eq!(design_sample("Snippet", None, 0), Value::Str("Snippet 1".into()));
        assert_eq!(design_sample("Label", Some(&crate::registry::PropKind::F32), 2), Value::F32(3.0), "the property's kind first");
    }
}
