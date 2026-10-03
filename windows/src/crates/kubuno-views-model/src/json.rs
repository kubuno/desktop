//! Reading a `kbview-registry.json` document (`vskubuno/docs/VIEWS-SPEC.md` §10): the desktop export
//! (`{version, components}`) or the web registry shipped in `@kubuno/ui`
//! (`{schema: 1, target: "web", version, typography, components}`).
//!
//! The types here own their strings (a registry read at run time is not `'static`, unlike the
//! compiled-in tables of [`crate::meta`]); their field names are those of [`crate::schema`], which
//! writes the same document. Unknown fields are ignored and absent ones take their default, so a
//! newer writer never breaks an older reader (§10.1). The `web` block and `typography` are kept as
//! raw JSON: only the web compiler, runtime and designer read them.

use serde::{Deserialize, Serialize};

use crate::schema::{ChildrenModelJson, LayoutKindJson};

/// The schema version this reader understands (`"schema"` of the web file; the desktop export
/// has no such field and is version 1).
pub const SCHEMA_VERSION: u32 = 1;

/// Why a registry document could not be read.
#[derive(Debug)]
pub enum LoadError {
    /// Not JSON, or not the shape of §10.
    Json(serde_json::Error),
    /// A `schema` newer than [`SCHEMA_VERSION`].
    UnsupportedSchema(u32),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "invalid element registry: {e}"),
            LoadError::UnsupportedSchema(v) => write!(f, "element registry schema {v} is not supported (this reader knows schema {SCHEMA_VERSION})"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Json(e) => Some(e),
            LoadError::UnsupportedSchema(_) => None,
        }
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(e: serde_json::Error) -> Self {
        LoadError::Json(e)
    }
}

/// A whole registry document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryDocument {
    /// `1` for the web file; absent (`None`) in the desktop export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<u32>,
    /// `"web"` for the web file; absent in the desktop export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The cache key (64-bit FNV-1a, hex).
    pub version: String,
    /// The web typography contract (§10.6), raw.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typography: Option<serde_json::Value>,
    pub components: Vec<ComponentEntry>,
}

impl RegistryDocument {
    /// The element named `name` (case-sensitive, like element names).
    pub fn component(&self, name: &str) -> Option<&ComponentEntry> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Whether this is the web registry (`"target": "web"`).
    pub fn is_web(&self) -> bool {
        self.target.as_deref() == Some("web")
    }
}

/// Reads a registry document from its JSON text.
pub fn load_registry(json: &str) -> Result<RegistryDocument, LoadError> {
    let doc: RegistryDocument = serde_json::from_str(json)?;
    check_schema(doc)
}

/// Reads a registry document from JSON bytes (a file read as is).
pub fn load_registry_slice(json: &[u8]) -> Result<RegistryDocument, LoadError> {
    let doc: RegistryDocument = serde_json::from_slice(json)?;
    check_schema(doc)
}

fn check_schema(doc: RegistryDocument) -> Result<RegistryDocument, LoadError> {
    match doc.schema {
        Some(v) if v > SCHEMA_VERSION => Err(LoadError::UnsupportedSchema(v)),
        _ => Ok(doc),
    }
}

/// A property's value kind (§10.3): serde's externally tagged encoding, `"Bool"` … `{"Enum": [...]}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropKindEntry {
    Bool,
    F32,
    #[default]
    String,
    Enum(Vec<String>),
}

/// One element (§10.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentEntry {
    pub name: String,
    pub doc: String,
    pub doc_fr: Option<String>,
    pub family: String,
    pub icon: String,
    pub children: ChildrenModelJson,
    pub allowed_children: Vec<String>,
    pub layout_kind: Option<LayoutKindJson>,
    pub properties: Vec<PropertyEntry>,
    pub events: Vec<EventEntry>,
    pub default_event: Option<String>,
    pub base_chain: Vec<String>,
    pub origin: String,
    pub kind: String,
    pub non_visual: bool,
    pub linked: bool,
    pub extends: Option<String>,
    pub crate_name: Option<String>,
    pub toolbox_category: Option<String>,
    pub toolbox_icon: Option<String>,
    pub browsable: bool,
    pub default_property: Option<String>,
    pub view_path: Option<String>,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
    pub design_size: Option<[f32; 2]>,
    /// What the designer writes when the element is added from the Toolbox (web; desktop: future).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_defaults: Option<serde_json::Value>,
    /// The web block (§10.5), raw.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<serde_json::Value>,
}

impl Default for ComponentEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            doc: String::new(),
            doc_fr: None,
            family: String::new(),
            icon: String::new(),
            children: ChildrenModelJson::None,
            allowed_children: Vec::new(),
            layout_kind: None,
            properties: Vec::new(),
            events: Vec::new(),
            default_event: None,
            base_chain: Vec::new(),
            origin: String::new(),
            kind: String::new(),
            non_visual: false,
            linked: false,
            extends: None,
            crate_name: None,
            toolbox_category: None,
            toolbox_icon: None,
            // An element is offered in the Toolbox unless it says otherwise.
            browsable: true,
            default_property: None,
            view_path: None,
            source_file: None,
            source_line: None,
            design_size: None,
            design_defaults: None,
            web: None,
        }
    }
}

impl ComponentEntry {
    /// The property whose attribute is `name` or one of its aliases (the export already lists the
    /// inherited and root-only ones on every element).
    pub fn property(&self, name: &str) -> Option<&PropertyEntry> {
        self.properties.iter().find(|p| p.name == name || p.aliases.iter().any(|a| a == name))
    }

    /// The event whose attribute is `name` or one of its aliases.
    pub fn event(&self, name: &str) -> Option<&EventEntry> {
        self.events.iter().find(|e| e.name == name || e.aliases.iter().any(|a| a == name))
    }

    /// Whether the element's class is `class` or derives from it.
    pub fn is_a(&self, class: &str) -> bool {
        self.base_chain.iter().any(|c| c == class)
    }
}

/// One property (§10.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropertyEntry {
    pub name: String,
    pub kind: PropKindEntry,
    pub default: String,
    pub doc: String,
    pub doc_fr: Option<String>,
    pub category: Option<String>,
    pub browsable: bool,
    pub bindable: bool,
    pub localizable: bool,
    pub serialization: Option<String>,
    pub editor: Option<String>,
    pub type_converter: Option<String>,
    pub inherited_from: Option<String>,
    pub root_only: bool,
    pub design_time: bool,
    pub aliases: Vec<String>,
}

impl Default for PropertyEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: PropKindEntry::String,
            default: String::new(),
            doc: String::new(),
            doc_fr: None,
            category: None,
            browsable: true,
            bindable: false,
            localizable: false,
            serialization: None,
            editor: None,
            type_converter: None,
            inherited_from: None,
            root_only: false,
            design_time: false,
            aliases: Vec::new(),
        }
    }
}

impl PropertyEntry {
    /// The editor as a closed type ([`crate::EditorKind`]).
    pub fn editor_kind(&self) -> Option<crate::EditorKind<'_>> {
        self.editor.as_deref().map(crate::EditorKind::parse)
    }
}

/// One event (§10.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventEntry {
    pub name: String,
    pub display_name: String,
    pub doc: String,
    pub doc_fr: Option<String>,
    pub category: String,
    pub args_type: String,
    pub args_chain: Vec<String>,
    pub args_rust_type: String,
    pub args_mut: bool,
    pub cancelable: bool,
    pub routing: String,
    pub aliases: Vec<String>,
    pub browsable: bool,
    pub root_only: bool,
    pub common: bool,
    pub inherited_from: Option<String>,
}

impl Default for EventEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            doc: String::new(),
            doc_fr: None,
            category: String::new(),
            args_type: "EventArgs".to_string(),
            args_chain: vec!["EventArgs".to_string()],
            args_rust_type: "EmptyEventArgs".to_string(),
            args_mut: false,
            cancelable: false,
            routing: "Direct".to_string(),
            aliases: Vec::new(),
            browsable: true,
            root_only: false,
            common: false,
            inherited_from: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: &str = r#"{"version":"0123456789abcdef","components":[
        {"name":"Button","doc":"A button.","doc_fr":"Un bouton.","family":"core","icon":"button","children":"None",
         "allowed_children":[],"layout_kind":null,
         "properties":[{"name":"Text","kind":"String","default":"","doc":"The caption.","doc_fr":null,"category":"Appearance",
                        "browsable":true,"bindable":false,"localizable":true,"serialization":null,"editor":null,
                        "type_converter":null,"inherited_from":null,"root_only":false,"design_time":false,"aliases":["Label"]},
                       {"name":"Variant","kind":{"Enum":["Primary","Secondary"]},"default":"Primary","doc":"","doc_fr":null,
                        "category":"Appearance","browsable":true,"bindable":false,"localizable":false,"serialization":null,
                        "editor":null,"type_converter":null,"inherited_from":null,"root_only":false,"design_time":false,"aliases":[]},
                       {"name":"ContextMenu","kind":"String","default":"","doc":"","editor":"reference:ContextMenu","inherited_from":"Control"}],
         "events":[{"name":"OnClick","display_name":"Click","doc":"","doc_fr":null,"category":"Action","args_type":"MouseEventArgs",
                    "args_chain":["MouseEventArgs","EventArgs"],"args_rust_type":"MouseEventArgs","args_mut":false,"cancelable":false,
                    "routing":"Direct","aliases":["OnPressed"],"browsable":true,"root_only":false,"common":false,"inherited_from":null}],
         "default_event":"OnClick","base_chain":["Button","ButtonBase","Control","Component"],"origin":"builtin","kind":"control",
         "non_visual":false,"linked":false,"extends":null,"crate_name":null,"toolbox_category":null,"toolbox_icon":null,
         "browsable":true,"default_property":null,"view_path":null,"source_file":null,"source_line":null,"design_size":null},
        {"name":"Stack","children":"List","allowed_children":[],"layout_kind":"Flow"}]}"#;

    #[test]
    fn reads_the_desktop_export_shape() {
        let doc = load_registry(DESKTOP).expect("the desktop shape loads");
        assert_eq!(doc.version, "0123456789abcdef");
        assert!(!doc.is_web());
        assert_eq!(doc.schema, None);
        let button = doc.component("Button").expect("Button");
        assert_eq!(button.doc_fr.as_deref(), Some("Un bouton."));
        assert!(button.is_a("ButtonBase"));
        assert_eq!(button.property("Label").map(|p| p.name.as_str()), Some("Text"));
        assert_eq!(button.property("Variant").map(|p| &p.kind), Some(&PropKindEntry::Enum(vec!["Primary".into(), "Secondary".into()])));
        assert_eq!(button.property("ContextMenu").and_then(PropertyEntry::editor_kind), Some(crate::EditorKind::Reference("ContextMenu")));
        assert_eq!(button.event("OnPressed").map(|e| e.display_name.as_str()), Some("Click"));
        let stack = doc.component("Stack").expect("Stack");
        assert_eq!(stack.children, ChildrenModelJson::List);
        assert_eq!(stack.layout_kind, Some(LayoutKindJson::Flow));
        // Absent fields take their defaults.
        assert!(stack.browsable);
        assert!(stack.properties.is_empty());
    }

    #[test]
    fn reads_the_web_shape_and_ignores_unknown_fields() {
        let json = r#"{"schema":1,"target":"web","version":"ffff","typography":{"roles":{}},"future":42,
            "components":[{"name":"Switch","children":"None","web":{"module":"@ui","export":"Toggle"},"later":true,
                           "properties":[{"name":"Checked","kind":"Bool","bindable":true,"web_note":"x"}]}]}"#;
        let doc = load_registry(json).expect("the web shape loads");
        assert!(doc.is_web());
        assert!(doc.typography.is_some());
        let switch = doc.component("Switch").expect("Switch");
        assert_eq!(switch.web.as_ref().and_then(|w| w.get("export")).and_then(|e| e.as_str()), Some("Toggle"));
        assert_eq!(switch.property("Checked").map(|p| (&p.kind, p.bindable)), Some((&PropKindEntry::Bool, true)));
    }

    #[test]
    fn rejects_a_newer_schema_and_malformed_json() {
        assert!(matches!(load_registry(r#"{"schema":2,"version":"","components":[]}"#), Err(LoadError::UnsupportedSchema(2))));
        assert!(matches!(load_registry("[1,2"), Err(LoadError::Json(_))));
        assert!(matches!(load_registry_slice(br#"{"components":[{"name":"X","children":"Many"}]}"#), Err(LoadError::Json(_))));
    }

    #[test]
    fn round_trips_through_its_own_serialization() {
        let doc = load_registry(DESKTOP).expect("loads");
        let text = serde_json::to_string(&doc).expect("serializes");
        assert_eq!(load_registry(&text).expect("reloads"), doc);
    }
}
