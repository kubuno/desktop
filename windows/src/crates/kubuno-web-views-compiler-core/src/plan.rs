//! The render plan: what `@kubuno/views` renders (VIEWS-SPEC §10, `WEB-VIEWS.md` §2.1 and §13 "WV-2 as
//! built"). Pure data, serialised as JSON: the browser designer feeds it to the renderer as is (bindings
//! evaluated by a path walker), the Vite plugin turns it into a JS module where components and icons are
//! imported and every binding gets a precompiled accessor — the only difference between the two paths.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::registry::{EventSource, PropTarget};

/// The plan format. Bumped on an incompatible change; a host whose `@kubuno/views` runtime reads another
/// ABI refuses the module (`viewsAbi`, WEB-VIEWS §6.4).
pub const VIEWS_ABI: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub abi: u32,
    /// The view file, relative to the project root, `/`-separated.
    pub file: String,
    /// `view` (`.kbview`) or `control` (`.kbcontrol`).
    pub kind: &'static str,
    pub root: Node,
    /// Non-visual components (`ContextMenu`, `ToolTip`, `Timer`, `Query`…), the designer's tray.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tray: Vec<Node>,
    /// `x:Name` → element id.
    pub names: BTreeMap<String, String>,
    /// Every handler the view names.
    pub handlers: Vec<String>,
    /// Design builds only: `DesignWidth` / `DesignHeight`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_size: Option<[f64; 2]>,
}

/// One element.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Node {
    /// Element id (`Element::stable_id`: dot path of element-child ordinals, `""` = root).
    pub id: String,
    /// Element name (`Button`).
    pub el: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 1-based line / UTF-16 column of the start tag.
    pub at: [u32; 2],
    /// Import specifier and named export of the component (`None` for items and runtime-only elements).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// How the designer reaches the root DOM node (`ref`, `wrapper`, `portal`, `none`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom: Option<String>,
    /// `user_control` for a `.kbcontrol` element (props passed through as `this.props`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub fixed: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<Prop>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    /// Child elements: rendered into `content`, or the items of `items`, or (an item's own) content.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// The prop receiving the rendered children (`children`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Property elements (`<Card.Actions>`): React prop → rendered nodes.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, Vec<Node>>,
    /// The children → prop adapter of this element, with its items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Items>,
    /// The children are an item template (`Repeater`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub template: bool,
    /// Size-class values (`Direction.Expanded`): class → properties.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sc: BTreeMap<String, Vec<Prop>>,
    /// Design builds only: `d:` values, property → value.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub design: Vec<Prop>,
}

/// The adapter that turns child items into a prop (`Option` children → `Dropdown.options`).
#[derive(Debug, Clone, Serialize)]
pub struct Items {
    pub prop: String,
    /// `array` or `record`.
    pub shape: String,
    /// Where an item's own content goes: `{"field": "content"}`, `"selected-after"`, `"none"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nested: Option<String>,
    pub list: Vec<Node>,
}

/// One property as written, with where it goes and its value.
#[derive(Debug, Clone, Serialize)]
pub struct Prop {
    /// The canonical property name (`Text`, `Stack.Fill`).
    pub n: String,
    /// Where it goes (`prop` / `field` / `runtime`, `convert`, `values`, `change`).
    pub to: PropTarget,
    /// The literal value, typed by the property's kind and mapped through `values`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<Binding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub res: Option<Res>,
    /// The property's kind (`Bool`, `F32`, `String`, `Enum`), for the runtime's binding conversions.
    pub kind: &'static str,
    /// 1-based line / UTF-16 column of the attribute.
    pub at: [u32; 2],
}

/// A `{Binding …}` (VIEWS-SPEC §6.1).
#[derive(Debug, Clone, Serialize)]
pub struct Binding {
    pub path: String,
    /// `OneWay`, `TwoWay`, `OneTime`, `OneWayToSource`.
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub null: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub culture: Option<String>,
    /// Template depth the path is evaluated at (0 = the view; n = the n-th enclosing `Repeater` row
    /// first, then outwards).
    #[serde(skip_serializing_if = "is_zero")]
    pub depth: u32,
    /// Where the path is written: 1-based line / UTF-16 column (source maps of the accessors).
    pub at: [u32; 2],
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// A `{Res key[, Source=set][, Name=value]…}` (`WEB-VIEWS.md` §2.4, WV-6). Without arguments the JSON is the
/// same as before WV-6 (`args` is omitted): the change is additive, `VIEWS_ABI` stays 1.
#[derive(Debug, Clone, Serialize)]
pub struct Res {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    /// The arguments, in source order: they fill the string's `{{name}}` placeholders, and `Count`
    /// (case-insensitive, numeric) selects the plural form (i18next's `count`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<ResArgPlan>,
}

/// One argument of a `{Res}`: a one-way binding (`b`, same rules as a property binding: path, depth inside
/// templates, `at`) or a literal (`v`).
#[derive(Debug, Clone, Serialize)]
pub struct ResArgPlan {
    /// The name as written (`Count`, `Name`).
    pub n: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<Binding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<String>,
}

/// One event attribute: the handler and where the event comes from.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Canonical event name (`OnClick`).
    pub n: String,
    /// The code-behind method.
    pub h: String,
    pub from: EventSource,
    /// The args type (`MouseEventArgs`).
    pub args_type: String,
    pub at: [u32; 2],
}
