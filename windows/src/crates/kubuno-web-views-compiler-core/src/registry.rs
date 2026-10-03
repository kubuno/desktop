//! The web element registry as the compiler reads it: the host file (`kbview-registry.web.json`, shipped
//! by `@kubuno/ui`) plus the project's own registries (custom controls) and user controls (`.kbcontrol`).
//!
//! The JSON is the shared schema of `vskubuno/docs/VIEWS-SPEC.md` §10, read by
//! `kubuno_desktop_views_model::load_registry`; the `web` block (§10.5), kept raw by the model, is typed here.

use std::collections::{BTreeMap, HashMap};

use kubuno_desktop_views_model::{ComponentEntry, PropertyEntry, RegistryDocument};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The import specifiers a view may reach outside its own project: the host singletons resolved by the
/// import map (VIEWS-SPEC "Module isolation" rule 1). Anything else that is not project-local is rejected.
pub const HOST_MODULES: &[&str] = &["@ui", "@kubuno/sdk", "@kubuno/drive", "@kubuno/views"];

/// Whether `module` is a project-local specifier: `./x`, `../x` (relative to the view) or `/x` (relative
/// to the project root, rewritten by the Vite plugin).
pub fn is_project_module(module: &str) -> bool {
    module.starts_with("./") || module.starts_with("../") || module.starts_with('/')
}

/// How one `.kbview` property reaches the component (VIEWS-SPEC §10.5 `prop_map`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PropTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convert: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    /// Filled by the compiler (never read from a registry): the source of the `change` event, so a two-way
    /// binding can write back even when the view does not handle that event itself.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub change_from: Option<EventSource>,
}

/// Where one `.kbview` event comes from (VIEWS-SPEC §10.5 `event_map`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(default)]
pub struct EventSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    pub args: String,
}

/// One name or several (`item: "Option"` / `item: ["A", "B"]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Names {
    One(String),
    Many(Vec<String>),
}

impl Default for Names {
    fn default() -> Self {
        Names::Many(Vec::new())
    }
}

impl Names {
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Names::One(n) => n == name,
            Names::Many(v) => v.iter().any(|n| n == name),
        }
    }

    pub fn list(&self) -> Vec<String> {
        match self {
            Names::One(n) => vec![n.clone()],
            Names::Many(v) => v.clone(),
        }
    }
}

/// The children → prop adapter of a parent element (VIEWS-SPEC §10.5 `children_to_prop`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ChildrenToProp {
    pub prop: String,
    pub shape: Option<String>,
    pub item: Names,
    pub content: Option<Value>,
    pub key: Option<String>,
    pub nested: Option<String>,
}

/// A component that renders the element for some literal property values (`TextField Variant="Outlined"`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Alternate {
    pub when: BTreeMap<String, String>,
    pub module: Option<String>,
    pub export: Option<String>,
    pub dom_root: Option<String>,
    pub prop_map: BTreeMap<String, PropTarget>,
    pub event_map: BTreeMap<String, EventSource>,
    pub fixed: Map<String, Value>,
}

/// The `web` block of an element (VIEWS-SPEC §10.5).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WebBlock {
    pub module: Option<String>,
    pub export: Option<String>,
    pub dom_root: Option<String>,
    pub prop_map: BTreeMap<String, PropTarget>,
    pub event_map: BTreeMap<String, EventSource>,
    pub children_to_prop: Option<ChildrenToProp>,
    pub content: Option<String>,
    pub slots: BTreeMap<String, String>,
    pub alternates: Vec<Alternate>,
    pub item_of: Vec<String>,
    pub fixed: Map<String, Value>,
    pub web_only: Vec<String>,
    /// Web compiler extension: the element's children are an item template instantiated once per item of
    /// `ItemsSource` (`Repeater`); bindings inside resolve against the row first (VIEWS-SPEC §6.1).
    pub template: bool,
}

/// Where an element comes from — what the module isolation rule checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `@kubuno/ui` / `@kubuno/sdk` / `@kubuno/views` (the host registry).
    Host,
    /// A registry file of the project (custom controls).
    Project,
    /// A `.kbcontrol` of the project, used by its file stem.
    UserControl,
}

/// One element the compiler knows.
#[derive(Debug, Clone)]
pub struct Element {
    pub entry: ComponentEntry,
    pub web: WebBlock,
    pub origin: Origin,
    /// Which registry document declared it (for messages).
    pub source: String,
}

impl Element {
    /// An item consumed by its parent's adapter (`Option`, `TabItem`…): no component of its own.
    pub fn is_item(&self) -> bool {
        self.web.module.is_none() && !self.web.item_of.is_empty()
    }

    /// The property named `name` (or one of its aliases).
    pub fn property(&self, name: &str) -> Option<&PropertyEntry> {
        self.entry.property(name)
    }

    pub fn is_user_control(&self) -> bool {
        self.origin == Origin::UserControl
    }
}

/// A user control of the project: `<MessageRow/>` for `MessageRow.kbcontrol`, rendered by the default
/// export of its code-behind.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct UserControlRef {
    pub name: String,
    /// The code-behind module, project-local (`/src/MessageRow` or `./MessageRow`).
    pub module: String,
}

/// The merged registry: host first, then project registries, then user controls.
#[derive(Debug, Clone, Default)]
pub struct WebRegistry {
    elements: Vec<Element>,
    by_name: HashMap<String, usize>,
    /// Problems found while merging (duplicate names, isolation violations), reported by every compile.
    pub problems: Vec<String>,
}

impl WebRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a registry document. `host` documents may name host modules only; project documents may name
    /// host modules or project-local ones. A name already known is never overridden (first wins) and is
    /// reported — a project control cannot shadow a host element.
    pub fn add_document(&mut self, doc: &RegistryDocument, label: &str, host: bool) {
        for entry in &doc.components {
            let web: WebBlock = match entry.web.clone().map(serde_json::from_value::<WebBlock>) {
                Some(Ok(w)) => w,
                Some(Err(e)) => {
                    self.problems.push(format!("{label}: element `{}` has an invalid `web` block: {e}", entry.name));
                    continue;
                }
                None => {
                    // A desktop-only element (no `web` block): not available on this target.
                    continue;
                }
            };
            for module in std::iter::once(web.module.as_deref()).chain(web.alternates.iter().map(|a| a.module.as_deref())).flatten() {
                let allowed = HOST_MODULES.contains(&module) || (!host && is_project_module(module));
                if !allowed {
                    self.problems.push(format!(
                        "{label}: element `{}` imports `{module}`, which is neither a host singleton ({}) nor a file of this project — a view may only use host elements and its own module's controls (module isolation)",
                        entry.name,
                        HOST_MODULES.join(", ")
                    ));
                }
            }
            let origin = if host { Origin::Host } else { Origin::Project };
            self.insert(Element { entry: entry.clone(), web, origin, source: label.to_string() });
        }
    }

    /// Adds the user controls of the project.
    pub fn add_user_controls(&mut self, controls: &[UserControlRef]) {
        for uc in controls {
            if !is_project_module(&uc.module) {
                self.problems.push(format!(
                    "user control `{}` is rendered by `{}`, which is not a file of this project (module isolation)",
                    uc.name, uc.module
                ));
                continue;
            }
            let entry = ComponentEntry {
                name: uc.name.clone(),
                family: "project".into(),
                origin: "project".into(),
                kind: "user_control".into(),
                base_chain: vec![uc.name.clone(), "UserControl".into(), "Control".into(), "Component".into()],
                children: kubuno_desktop_views_model::schema::ChildrenModelJson::None,
                ..ComponentEntry::default()
            };
            let web = WebBlock {
                module: Some(uc.module.clone()),
                export: Some("default".into()),
                dom_root: Some("wrapper".into()),
                ..WebBlock::default()
            };
            self.insert(Element { entry, web, origin: Origin::UserControl, source: format!("{}.kbcontrol", uc.name) });
        }
    }

    fn insert(&mut self, element: Element) {
        if let Some(&existing) = self.by_name.get(&element.entry.name) {
            let first = &self.elements[existing];
            self.problems.push(format!(
                "`{}` is declared twice ({} and {}); the first one is used",
                element.entry.name, first.source, element.source
            ));
            return;
        }
        self.by_name.insert(element.entry.name.clone(), self.elements.len());
        self.elements.push(element);
    }

    pub fn get(&self, name: &str) -> Option<&Element> {
        self.by_name.get(name).map(|&i| &self.elements[i])
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.elements.iter().map(|e| e.entry.name.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.elements.iter()
    }
}
