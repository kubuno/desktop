//! The wire shape of the element registry, `kbview-registry.json` version 1 (`vskubuno/docs/VIEWS-SPEC.md`
//! §10): what the desktop export (`kubuno_views::registry::export`, served as `kubuno/registry` and printed by
//! `view_embed --export-registry`) serializes. Moved here by WV-1 so the web tooling and the language server's
//! web profile share one definition with the desktop export; [`crate::json`] reads the same document back.
//!
//! Field names are snake_case; the desktop export keeps its exact key sets (its own tests assert them).

use serde::{Deserialize, Serialize};

use crate::{ChildrenModel, LayoutKind, PropKind};

/// One exported property — [`crate::PropertyMeta`], field-for-field
/// (`vskubuno/docs/DESIGNER.md` §5: "properties: [{name, kind, default,
/// doc}]").
#[derive(Debug, Serialize)]
pub struct PropertyJson {
    pub name: &'static str,
    pub kind: PropKindJson,
    pub default: &'static str,
    pub doc: &'static str,
    /// The French user documentation (`kubuno_views::registry::docs_fr`), or null.
    pub doc_fr: Option<&'static str>,
    /// The Properties window group a custom control declares (`#[category]`, EVT-7b), null for the
    /// built-in properties (the designer groups them).
    pub category: Option<&'static str>,
    /// Listed in the Properties window.
    pub browsable: bool,
    pub bindable: bool,
    pub localizable: bool,
    /// `"Visible"`, `"Hidden"`, `"Content"`, or null.
    pub serialization: Option<&'static str>,
    /// The Properties window editor (`"color"`…), or null.
    pub editor: Option<&'static str>,
    pub type_converter: Option<&'static str>,
    /// The level of the control hierarchy that declares the property when the element inherits it
    /// (`"Control"` for `Enabled`, `"View"` for the root-only view properties), null for its own.
    pub inherited_from: Option<&'static str>,
    /// A view property (the form's): accepted on the view's root element only.
    pub root_only: bool,
    /// Read by the designer only (`Locked`…).
    pub design_time: bool,
    /// Older attribute names still accepted for it.
    pub aliases: Vec<&'static str>,
}

/// Mirrors [`PropKind`] with serde's own default "externally tagged" enum
/// encoding — the three scalar variants as bare strings (`"Bool"`, `"F32"`,
/// `"String"`), the one variant that carries data as a single-key object
/// (`{"Enum": ["A", "B"]}`) — exactly what the C# `PropKindJsonConverter`'s
/// own doc comment already says it expects ("serde's default... not
/// something invented for this bridge"), so no `#[serde(...)]` attribute is
/// needed here at all.
#[derive(Debug, Serialize)]
pub enum PropKindJson {
    Bool,
    F32,
    String,
    Enum(Vec<&'static str>),
}

impl From<PropKind> for PropKindJson {
    fn from(kind: PropKind) -> Self {
        match kind {
            PropKind::Bool => PropKindJson::Bool,
            PropKind::F32 => PropKindJson::F32,
            PropKind::String => PropKindJson::String,
            PropKind::Enum(variants) => PropKindJson::Enum(variants.to_vec()),
        }
    }
}

/// One exported event — [`crate::EventMeta`], field-for-field (`vskubuno/docs/EVENTS.md`
/// §5.1, EVT-3): the attribute name, its display name (no `On`), the user docs, the ⚡
/// tab category, the args type and chain, cancelable, routing, older aliases, browsable,
/// and `root_only` for the view's own events (`kubuno_views::registry::VIEW_EVENTS`), which only the
/// root element accepts.
#[derive(Debug, Serialize)]
pub struct EventJson {
    pub name: &'static str,
    pub display_name: &'static str,
    pub doc: &'static str,
    /// The French user documentation (`kubuno_views::registry::docs_fr`), or null.
    pub doc_fr: Option<&'static str>,
    pub category: &'static str,
    pub args_type: &'static str,
    pub args_chain: Vec<&'static str>,
    /// The Rust args type a typed handler declares (`"MouseEventArgs"`, `"TextChangedEventArgs"`).
    pub args_rust_type: &'static str,
    /// A handler writes back into the args (`handled`/`cancel`): a typed stub takes `&mut`.
    pub args_mut: bool,
    pub cancelable: bool,
    pub routing: &'static str,
    pub aliases: Vec<&'static str>,
    pub browsable: bool,
    pub root_only: bool,
    /// One of `kubuno_views::registry::COMMON_EVENTS` (mouse, keys, focus…) rather than the component's own.
    pub common: bool,
    /// The level of the control hierarchy that declares the event when the element inherits it
    /// (`"Control"` for the common events, `"View"` for the root-only view events), null for the
    /// component's own (EVT-7a).
    pub inherited_from: Option<&'static str>,
}

/// Mirrors [`ChildrenModel`] as the bare string the C# `JsonStringEnumConverter`
/// expects (`ComponentMeta.Children`) — [`ChildrenModel::List`]'s own gated
/// names travel separately, as [`ComponentJson::allowed_children`] (see this
/// module's doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChildrenModelJson {
    None,
    SingleWidget,
    List,
}

impl From<ChildrenModel> for ChildrenModelJson {
    fn from(children: ChildrenModel) -> Self {
        match children {
            ChildrenModel::None => ChildrenModelJson::None,
            ChildrenModel::SingleWidget => ChildrenModelJson::SingleWidget,
            ChildrenModel::List(_) => ChildrenModelJson::List,
        }
    }
}

/// Mirrors [`LayoutKind`]'s real, current variant set — see this module's doc
/// for why this is wider than the C# type's first guess, and why that guess
/// was the thing that had to change, not this. [`LayoutKind::None`] itself
/// never appears here: [`ComponentJson::layout_kind`] is `Option<Self>`, and
/// `None` maps to `Option::None` (JSON `null`), matching what the C# side
/// already modelled for "no layout engine of its own". A plain function
/// rather than a `From<LayoutKind> for Option<LayoutKindJson>` impl: `Option`
/// is a foreign, non-fundamental type, so that impl would need an orphan-rule
/// exemption this crate does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutKindJson {
    Flow,
    DockAnchor,
    Split,
    Tabs,
}

impl LayoutKindJson {
    pub fn from_registry(layout: LayoutKind) -> Option<Self> {
        match layout {
            LayoutKind::None => None,
            LayoutKind::Flow => Some(LayoutKindJson::Flow),
            LayoutKind::DockAnchor => Some(LayoutKindJson::DockAnchor),
            LayoutKind::Split => Some(LayoutKindJson::Split),
            LayoutKind::Tabs => Some(LayoutKindJson::Tabs),
        }
    }
}

/// One exported component — the whole shape §5 asks for. Field order matches
/// the regenerated fixture's own (`name, doc, family, icon, children,
/// allowed_children, layout_kind, properties, events`); JSON object key order
/// is not semantically significant to either side's deserializer, but a
/// stable order keeps fixture diffs readable.
#[derive(Debug, Serialize)]
pub struct ComponentJson {
    pub name: &'static str,
    pub doc: &'static str,
    /// The French user documentation (`kubuno_views::registry::docs_fr`), or null.
    pub doc_fr: Option<&'static str>,
    /// The toolbox grouping — `"core"` for the five components declared
    /// directly in `registry/components.rs`, otherwise the owning
    /// `registry::families` module's own name (`"choice"`, `"containers"`,
    /// `"data"`, `"display"`, `"text"`), derived by name lookup against each
    /// family's `ALL` slice (`family_of` in the export) rather than a new field on
    /// `ComponentMeta` itself, per this package's scope (additive-only in
    /// `registry/mod.rs`; the family tables stay untouched).
    pub family: &'static str,
    /// A short glyph name for the toolbox (§5: "an icon glyph name...
    /// node.rs's static_icon table is the right precedent for a short
    /// curated name list"). Cheap here means mechanical, not hand-curated:
    /// the component's own PascalCase name, kebab-cased (`kebab_case` in the export) —
    /// `"TextField"` -> `"text-field"`, matching the one multi-word example
    /// the original illustrative fixture already used, with no
    /// per-component table to keep in sync as new components are
    /// registered.
    pub icon: String,
    pub children: ChildrenModelJson,
    /// [`ChildrenModel::List`]'s gated child names (see this module's doc);
    /// empty for every other component and for an ungated `List` container.
    pub allowed_children: Vec<&'static str>,
    pub layout_kind: Option<LayoutKindJson>,
    pub properties: Vec<PropertyJson>,
    /// The component's own events, then the common ones it does not override
    /// (`ComponentMeta::all_events`), then — for a control — the view events flagged
    /// `root_only`.
    pub events: Vec<EventJson>,
    /// The designer double-click's event (`ComponentMeta::default_event`); the view's
    /// root element uses `OnLoad` instead.
    pub default_event: Option<&'static str>,
    /// The element's class followed by its ancestors, `"Component"` last (EVT-7a):
    /// `["Button", "ButtonBase", "Control", "Component"]` (`ComponentMeta::base_chain`).
    pub base_chain: Vec<&'static str>,
    /// `"builtin"`, or `"project"` for an application's class (EVT-7b).
    pub origin: &'static str,
    /// `"control"`, `"user_control"` or `"component"` (non-visual: the designer's component tray).
    pub kind: &'static str,
    /// A non-visual component (see `kubuno_views::registry::is_non_visual`).
    pub non_visual: bool,
    /// The rest describes a project class (null/default for the built-in elements): compiled into
    /// the program that exported the registry (`true`), or only known from its source.
    pub linked: bool,
    pub extends: Option<&'static str>,
    pub crate_name: Option<&'static str>,
    pub toolbox_category: Option<&'static str>,
    pub toolbox_icon: Option<&'static str>,
    /// Offered in the Toolbox.
    pub browsable: bool,
    pub default_property: Option<&'static str>,
    pub view_path: Option<&'static str>,
    pub source_file: Option<&'static str>,
    pub source_line: Option<u32>,
    /// A user control's design size (its view's `DesignWidth` × `DesignHeight`): the size the designer gives
    /// it when it is added from the Toolbox, as Windows Forms uses a UserControl's own `Size`.
    pub design_size: Option<[f32; 2]>,
}

/// The whole `kubuno/registry` result: the component array plus a `version`
/// the client can cache against (see this module's doc for why `version`
/// lives here and not inside the array itself).
#[derive(Debug, Serialize)]
pub struct RegistryExport {
    pub version: String,
    pub components: Vec<ComponentJson>,
}

/// A 64-bit FNV-1a hash, hex-encoded: what backs [`RegistryExport::version`] — cheap, no new
/// dependency, and deterministic across processes (unlike `DefaultHasher`'s per-process seed), which
/// a cache key shared by several language-server processes needs. A cache key, not a checksum.
pub fn fnv1a_hex(bytes: &[u8]) -> String {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}
