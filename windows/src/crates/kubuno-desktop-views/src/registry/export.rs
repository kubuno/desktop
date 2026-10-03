//! JSON export of the component registry for the Visual Studio designer
//! (`vskubuno/docs/DESIGNER.md` §5 "Registry export for the C# side", §8's
//! DSG-1) — served over the language server's `kubuno/registry` custom LSP
//! method (`kubuno-desktop-views-ls/src/server.rs`) and consumed by
//! `Kubuno.VisualStudio.Designer.Registry.ComponentRegistry.FromJson`
//! (`vskubuno/src/Kubuno.VisualStudio.Designer/Registry/ComponentRegistry.cs`),
//! whose fixture (`vskubuno/tests/Kubuno.VisualStudio.Designer.Tests/Fixtures/
//! registry.sample.json`) this module's own [`components_json`] output is
//! meant to equal — "both sides share one truth" rather than a fixture that
//! quietly drifts from what this crate actually exports.
//!
//! ## Where this module follows the C# side instead of §5's own sketch
//!
//! §5 sketches `kind`/`children`/`family` but leaves field-name casing and a
//! few field shapes unstated; the C# consumer (written first, against that
//! sketch) already committed to specific choices this module matches exactly
//! rather than re-deriving:
//!
//! - **snake_case field names** (`layout_kind`, not `layoutKind`) — the
//!   consumer's `Serialization/RegistryJsonOptions.cs` uses a
//!   `SnakeCaseNamingPolicy`, the opposite convention from DSG-2's
//!   `kubuno/applyEdit` bridge (`edit_bridge.rs`), which is `camelCase`
//!   throughout. Two different, already-shipped conventions for two
//!   different methods; this module matches its own consumer, not the other
//!   bridge's.
//! - **`LayoutKind` widened past the C# type's original 4 guessed
//!   variants.** The C# `Registry/LayoutKind.cs` enum (`Anchor`, `Dock`,
//!   `Flow`, `Split`) was written before [`super::LayoutKind`] existed on the
//!   Rust side (§4's gap it was meant to fill); the real enum that eventually
//!   landed does not split `Dock`/`Anchor` into two variants (one engine,
//!   [`super::LayoutKind::DockAnchor`], handles both per-child, not per-
//!   container) and adds a fifth, [`super::LayoutKind::Tabs`]. This module
//!   serializes the real five-variant enum's real names (`None` folded into
//!   JSON `null`, same as the C# side already expected); the C# enum was
//!   widened to match (`DockAnchor`, `Tabs` added, the guessed `Anchor`/`Dock`
//!   split dropped) as the minimal fix the real schema required.
//! - **`allowed_children` is a new, additive field**, not in §5's sketch or
//!   the original C# type: [`super::ChildrenModel::List`]'s gated child names
//!   (task requirement: "children model incl. allowed child names"). Emitted
//!   as its own array (empty when the container gates nothing) rather than
//!   folded into `children` itself, because `children` must stay the bare
//!   string `"List"` the C# `JsonStringEnumConverter` expects — a JSON object
//!   there would break deserialization. `System.Text.Json`'s default
//!   `Deserialize` silently ignores JSON properties a type does not declare,
//!   so this would have been safe to add even without a C# change; a matching
//!   `ComponentMeta.AllowedChildren` was added anyway so the data is usable,
//!   not just present.
//! - **The registry version/hash lives on the RPC envelope, not inside the
//!   component array.** `ComponentRegistry.FromJson` deserializes a bare
//!   JSON array (`List<ComponentMeta>`) — the fixture's own top-level shape.
//!   Wrapping that array in `{version, components}` would break `FromJson`
//!   outright, so [`RegistryExport`] (what `kubuno/registry` actually
//!   returns) wraps the array with a `version` field for the client to cache
//!   against, while [`components_json`] — what the fixture equals — stays
//!   the bare array `FromJson` already expects.
//!
//! ## What `version` is
//!
//! A 64-bit FNV-1a hash of the exported components' own canonical JSON
//! bytes, hex-encoded — cheap, no new crate dependency (a cache key only
//! ever needs to change when the content changes, never to resist
//! tampering), and deterministic across runs of the same binary, unlike
//! `std::collections::hash_map::DefaultHasher` (`RandomState`'s per-process
//! random seed would make two different `kubuno-desktop-views-ls` processes serving
//! the identical registry disagree on its hash, defeating the whole point of
//! a cache key).

use super::{ChildrenModel, ComponentMeta};

// The wire types live in the platform-neutral `kubuno-desktop-views-model` (`schema`, WV-1), beside the
// loader that reads this export back; re-exported here under their historical paths.
pub use kubuno_desktop_views_model::schema::{ChildrenModelJson, ComponentJson, EventJson, LayoutKindJson, PropKindJson, PropertyJson, RegistryExport};
use kubuno_desktop_views_model::schema::fnv1a_hex;

/// One exported property — [`super::PropertyMeta`] field-for-field, with its French documentation
/// and default category filled in.
fn property_json(component: &str, p: &super::PropertyMeta, inherited_from: Option<&'static str>, root_only: bool) -> PropertyJson {
    PropertyJson {
        name: p.name,
        kind: p.kind.into(),
        default: p.default,
        doc: p.doc,
        doc_fr: super::docs_fr::property_french(component, p.name, inherited_from),
        category: Some(p.category.unwrap_or_else(|| super::common::default_category(p.name, p.kind))),
        browsable: p.browsable,
        bindable: p.bindable,
        localizable: p.localizable,
        serialization: p.serialization,
        editor: p.editor,
        type_converter: p.type_converter,
        inherited_from,
        root_only,
        design_time: p.design_time,
        aliases: p.aliases.to_vec(),
    }
}

/// One exported event — [`super::EventMeta`] field-for-field, with where it comes from (own, common,
/// view) and its French documentation.
fn event_json(component: &super::ComponentMeta, e: &super::EventMeta, root_only: bool) -> EventJson {
    let common = !root_only && !component.events.iter().any(|own| own.name == e.name);
    let inherited_from = if root_only { Some("View") } else if common { component.inherited_event(e.name).map(|(level, _)| level) } else { None };
    let component = component.name;
    EventJson {
        name: e.name,
        display_name: e.display_name(),
        doc: e.doc,
        doc_fr: super::docs_fr::event_french(component, e.name),
        category: e.category.name(),
        args_type: e.args_type,
        args_chain: e.args_chain.to_vec(),
        args_rust_type: e.args_rust,
        args_mut: e.args_mut,
        cancelable: e.cancelable,
        routing: match e.routing {
            super::Routing::Direct => "Direct",
            super::Routing::Bubble => "Bubble",
        },
        aliases: e.aliases.to_vec(),
        browsable: e.browsable,
        root_only,
        common,
        inherited_from,
    }
}

/// Converts one PascalCase Rust type name into kebab-case (`"TextField"` ->
/// `"text-field"`, `"Button"` -> `"button"`) — see [`ComponentJson::icon`].
fn kebab_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.char_indices() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The toolbox family name for `name` — `"core"` for the five phase-2a
/// examples, else the enabled family whose `ALL` slice declares it, else
/// (never expected in practice — [`super::all`]'s own tests already assert
/// every registered name is unique and findable) `"other"`, a safe fallback
/// rather than a panic.
fn family_of(name: &str) -> &'static str {
    if super::components::ALL.iter().any(|c| c.name == name) {
        return "core";
    }
    #[cfg(feature = "family-display")]
    if super::families::display::ALL.iter().any(|c| c.name == name) {
        return "display";
    }
    #[cfg(feature = "family-choice")]
    if super::families::choice::ALL.iter().any(|c| c.name == name) {
        return "choice";
    }
    #[cfg(feature = "family-text")]
    if super::families::text::ALL.iter().any(|c| c.name == name) {
        return "text";
    }
    #[cfg(feature = "family-containers")]
    if super::families::containers::ALL
        .iter()
        .any(|c| c.name == name)
    {
        return "containers";
    }
    #[cfg(feature = "family-data")]
    if super::families::data::ALL.iter().any(|c| c.name == name) {
        return "data";
    }
    #[cfg(feature = "family-docking")]
    if super::families::docking::ALL.iter().any(|c| c.name == name) {
        return "docking";
    }
    // The newer families join the tabs their elements belong with: a list of views next to the
    // other lists, pictures next to the other displays, a grid and a popover with the containers.
    #[cfg(feature = "family-items")]
    if super::families::items::ALL.iter().any(|c| c.name == name) {
        return "data";
    }
    #[cfg(feature = "family-navigation")]
    if super::families::navigation::ALL.iter().any(|c| c.name == name) {
        return "navigation";
    }
    #[cfg(feature = "family-media")]
    if super::families::media::ALL.iter().any(|c| c.name == name) {
        return "display";
    }
    #[cfg(feature = "family-overlays")]
    if super::families::overlays::ALL.iter().any(|c| c.name == name) {
        return "containers";
    }
    #[cfg(feature = "family-layout")]
    if super::families::layout::ALL.iter().any(|c| c.name == name) {
        return "containers";
    }
    if super::families::ribbon::ALL.iter().any(|c| c.name == name) {
        return "ribbon";
    }
    // The menus (MENUS.md): their own Toolbox tab, the context menu and its items included (WinForms'
    // « Menus & Toolbars »).
    if super::families::menus::ALL.iter().any(|c| c.name == name) || matches!(name, "ContextMenu" | "MenuItem") {
        return "menus";
    }
    if super::families::components::ALL.iter().any(|c| c.name == name) {
        return "components";
    }
    "other"
}

fn french(component: &str, member: Option<&str>) -> Option<&'static str> {
    super::docs_fr::french(&super::docs_fr::key(component, member))
}

fn to_json(meta: &ComponentMeta) -> ComponentJson {
    let project = super::project_info(meta.name);
    let allowed_children = match meta.children {
        ChildrenModel::List(allowed) => allowed.to_vec(),
        _ => Vec::new(),
    };
    ComponentJson {
        name: meta.name,
        doc: meta.doc,
        doc_fr: french(meta.name, None),
        family: if project.is_some() { "project" } else { family_of(meta.name) },
        icon: project.and_then(|p| p.toolbox_icon).map(str::to_string).unwrap_or_else(|| kebab_case(meta.name)),
        children: meta.children.into(),
        allowed_children,
        layout_kind: LayoutKindJson::from_registry(meta.layout),
        properties: {
            let mut properties: Vec<PropertyJson> =
                meta.all_properties().into_iter().map(|(level, p)| property_json(meta.name, p, level, false)).collect();
            // The view's own properties, for a control that is the root of a view (like the view events).
            if meta.has_common_events() {
                properties.extend(
                    super::VIEW_PROPERTIES
                        .iter()
                        .filter(|p| !meta.all_properties().iter().any(|(_, o)| o.name == p.name))
                        .map(|p| property_json(meta.name, p, Some("View"), true)),
                );
            }
            properties
        },
        events: {
            let mut events: Vec<EventJson> = meta.all_events().into_iter().map(|e| event_json(meta, e, false)).collect();
            if meta.has_common_events() {
                events.extend(super::VIEW_EVENTS.iter().map(|e| event_json(meta, e, true)));
            }
            events
        },
        default_event: meta.default_event(),
        base_chain: meta.base_chain().to_vec(),
        origin: if project.is_some() { "project" } else { "builtin" },
        kind: match project {
            Some(p) => p.kind.as_str(),
            None if super::is_non_visual(meta.name) => "component",
            None => "control",
        },
        non_visual: super::is_non_visual(meta.name),
        linked: project.is_some_and(|p| p.origin == super::Origin::Linked),
        extends: project.map(|p| p.extends),
        crate_name: project.and_then(|p| p.crate_name),
        toolbox_category: project.and_then(|p| p.toolbox_category),
        toolbox_icon: project.and_then(|p| p.toolbox_icon),
        browsable: project.is_none_or(|p| p.browsable),
        default_property: project.and_then(|p| p.default_property),
        view_path: project.and_then(|p| p.view_path),
        source_file: project.and_then(|p| p.source_file),
        source_line: project.and_then(|p| p.source_line),
        design_size: crate::design::user_control_design_size(meta.name).map(|(w, h)| [w, h]),
    }
}

/// Every registered component, exported in [`super::all`]'s own order — the
/// bare JSON array `ComponentRegistry.FromJson` deserializes, and what the
/// checked-in fixture (`vskubuno/tests/.../Fixtures/registry.sample.json`) is
/// regenerated from.
pub fn components_json() -> Vec<ComponentJson> {
    super::all().iter().map(to_json).collect()
}

/// Builds [`RegistryExport`]. `version` is computed from the components'
/// *own* serialized JSON, so it changes exactly when a client re-fetching it
/// would see different data — not, e.g., from `registry::all()`'s
/// `OnceLock`'s address, which would be stable but meaningless.
/// [`export`] as one JSON line (what a design surface prints for `--export-registry`: it links no
/// JSON crate of its own).
pub fn export_json() -> String {
    serde_json::to_string(&export()).unwrap_or_default()
}

pub fn export() -> RegistryExport {
    let components = components_json();
    let version = fnv1a_hex(&serde_json::to_vec(&components).unwrap_or_default());
    RegistryExport {
        version,
        components,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(components: &'a [ComponentJson], name: &str) -> &'a ComponentJson {
        components
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("`{name}` not exported"))
    }

    #[test]
    fn exports_exactly_the_registered_components() {
        let components = components_json();
        for meta in super::super::builtins() {
            assert!(components.iter().any(|c| c.name == meta.name), "{} not exported", meta.name);
        }
        assert!(components.len() >= super::super::builtins().len());
    }

    #[test]
    fn every_component_serializes_the_expected_top_level_keys() {
        // "A Rust test that the exported JSON deserializes against a copy of
        // the C# fixture's shape" (task requirement) — key presence, both
        // the fields the original C# `ComponentMeta` already declares and
        // the additive `allowed_children` this module adds.
        let value = serde_json::to_value(components_json()).expect("components should serialize");
        let array = value.as_array().expect("top-level export is a JSON array");
        assert!(!array.is_empty());
        let expected_keys: std::collections::BTreeSet<&str> = [
            "name",
            "doc",
            "doc_fr",
            "family",
            "icon",
            "children",
            "allowed_children",
            "layout_kind",
            "properties",
            "events",
            "default_event",
            "base_chain",
            "origin",
            "kind",
            "non_visual",
            "linked",
            "extends",
            "crate_name",
            "toolbox_category",
            "toolbox_icon",
            "browsable",
            "default_property",
            "view_path",
            "source_file",
            "source_line",
            "design_size",
        ]
        .into_iter()
        .collect();
        for component in array {
            let obj = component
                .as_object()
                .expect("each component is a JSON object");
            let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
            assert_eq!(keys, expected_keys, "unexpected key set for {component:?}");

            for prop in obj["properties"]
                .as_array()
                .expect("properties is an array")
            {
                let prop_obj = prop.as_object().expect("each property is a JSON object");
                let prop_keys: std::collections::BTreeSet<&str> =
                    prop_obj.keys().map(String::as_str).collect();
                assert_eq!(
                    prop_keys,
                    ["name", "kind", "default", "doc", "doc_fr", "category", "browsable", "bindable", "localizable", "serialization", "editor", "type_converter", "inherited_from", "root_only", "design_time", "aliases"]
                        .into_iter()
                        .collect::<std::collections::BTreeSet<_>>()
                );
            }
            for event in obj["events"].as_array().expect("events is an array") {
                let event_obj = event.as_object().expect("each event is a JSON object");
                let event_keys: std::collections::BTreeSet<&str> =
                    event_obj.keys().map(String::as_str).collect();
                assert_eq!(
                    event_keys,
                    [
                        "name",
                        "display_name",
                        "doc",
                        "doc_fr",
                        "category",
                        "args_type",
                        "args_chain",
                        "args_rust_type",
                        "args_mut",
                        "cancelable",
                        "routing",
                        "aliases",
                        "browsable",
                        "root_only",
                        "common",
                        "inherited_from",
                    ]
                        .into_iter()
                        .collect::<std::collections::BTreeSet<_>>()
                );
            }
        }
    }

    #[test]
    fn prop_kind_serializes_scalars_as_bare_strings_and_enum_as_an_object() {
        let components = components_json();
        let button = find(&components, "Button");
        let value = serde_json::to_value(button).expect("Button should serialize");

        let text_kind = value["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Text")
            .unwrap()["kind"]
            .clone();
        assert_eq!(text_kind, serde_json::json!("String"));

        let loading_kind = value["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Loading")
            .unwrap()["kind"]
            .clone();
        assert_eq!(loading_kind, serde_json::json!("Bool"));

        let variant_kind = value["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Variant")
            .unwrap()["kind"]
            .clone();
        assert_eq!(
            variant_kind,
            serde_json::json!({ "Enum": ["Primary", "Secondary", "Ghost", "Text", "Danger", "TextDanger"] })
        );
    }

    #[test]
    fn children_and_layout_kind_match_the_registry() {
        let components = components_json();

        let button = find(&components, "Button");
        assert!(matches!(button.children, ChildrenModelJson::None));
        assert!(button.layout_kind.is_none());
        assert!(button.allowed_children.is_empty());

        let card = find(&components, "Card");
        assert!(matches!(card.children, ChildrenModelJson::SingleWidget));
        assert!(card.layout_kind.is_none());

        let stack = find(&components, "Stack");
        assert!(matches!(stack.children, ChildrenModelJson::List));
        assert!(matches!(stack.layout_kind, Some(LayoutKindJson::Flow)));
        assert!(stack.allowed_children.is_empty(), "Stack gates nothing");
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn gated_list_containers_export_their_allowed_children() {
        let components = components_json();

        let tabs = find(&components, "Tabs");
        assert!(matches!(tabs.layout_kind, Some(LayoutKindJson::Tabs)));
        assert_eq!(tabs.allowed_children, vec!["TabItem"]);

        let panel = find(&components, "Panel");
        assert!(matches!(
            panel.layout_kind,
            Some(LayoutKindJson::DockAnchor)
        ));
    }

    #[test]
    fn family_matches_the_declaring_table() {
        let components = components_json();
        assert_eq!(find(&components, "Button").family, "core");
        assert_eq!(find(&components, "Stack").family, "core");
    }

    #[cfg(feature = "family-display")]
    #[test]
    fn family_matches_display_table() {
        let components = components_json();
        assert_eq!(find(&components, "Label").family, "display");
    }

    #[test]
    fn icon_is_kebab_cased_from_the_component_name() {
        assert_eq!(kebab_case("Button"), "button");
        assert_eq!(kebab_case("TextField"), "text-field");
        let components = components_json();
        assert_eq!(find(&components, "TextField").icon, "text-field");
    }

    #[test]
    fn events_carry_their_metadata_and_the_default_event() {
        let components = components_json();
        let switch = find(&components, "Switch");
        assert_eq!(switch.default_event, Some("OnCheckedChanged"));
        let checked = switch.events.iter().find(|e| e.name == "OnCheckedChanged").expect("OnCheckedChanged");
        assert_eq!(checked.display_name, "CheckedChanged");
        assert_eq!(checked.category, "Property Changed");
        assert_eq!(checked.aliases, vec!["OnToggled"]);
        assert_eq!(checked.args_chain, vec!["ValueChangedEventArgs", "EventArgs"]);
        assert!(!checked.common && !checked.root_only);
        let down = switch.events.iter().find(|e| e.name == "OnMouseDown").expect("common events are exported");
        assert!(down.common && down.category == "Mouse" && down.args_type == "MouseEventArgs" && down.doc_fr.is_some());
        let validating = switch.events.iter().find(|e| e.name == "OnValidating").expect("OnValidating");
        assert!(validating.cancelable);
        let load = switch.events.iter().find(|e| e.name == "OnLoad").expect("view events are exported");
        assert!(load.root_only && !load.common);
        assert_eq!(find(&components, "Button").default_event, Some("OnClick"));
    }

    /// EVT-7a: the chain and where each inherited event comes from.
    #[test]
    fn the_base_chain_and_inherited_events_are_exported() {
        let components = components_json();
        let button = find(&components, "Button");
        assert_eq!(button.base_chain, vec!["Button", "ButtonBase", "Control", "Component"]);
        let own = button.events.iter().find(|e| e.name == "OnClick").expect("OnClick");
        assert_eq!(own.inherited_from, None, "Button declares its own Click");
        let down = button.events.iter().find(|e| e.name == "OnMouseDown").expect("OnMouseDown");
        assert_eq!(down.inherited_from, Some("Control"));
        let load = button.events.iter().find(|e| e.name == "OnLoad").expect("OnLoad");
        assert_eq!(load.inherited_from, Some("View"));
        assert_eq!(find(&components, "TabItem").base_chain, vec!["TabItem", "Component"]);
        assert_eq!(find(&components, "Panel").base_chain, vec!["Panel", "ContainerBase", "ScrollableControl", "Control", "Component"]);
    }

    /// Regenerates the C# fixture (`vskubuno/tests/Kubuno.VisualStudio.Designer.Tests/Fixtures/
    /// registry.sample.json`) from the real export: `KUBUNO_REGISTRY_FIXTURE=<path> cargo test -p
    /// kubuno-desktop-views --lib write_registry_fixture -- --ignored`.
    #[test]
    #[ignore]
    fn write_registry_fixture() {
        let Some(path) = std::env::var_os("KUBUNO_REGISTRY_FIXTURE") else { return };
        let json = serde_json::to_string_pretty(&components_json()).expect("the registry serializes");
        std::fs::write(path, json + "\n").expect("the fixture is written");
    }

    #[test]
    fn version_is_deterministic_for_the_same_content() {
        // Other tests of this process register classes concurrently: compare two exports of the same
        // component set, and the version with the hash of its own content.
        let a = export();
        let b = export();
        if a.components.len() == b.components.len() {
            assert_eq!(a.version, b.version);
        }
        assert_eq!(a.version, fnv1a_hex(&serde_json::to_vec(&a.components).unwrap()));
        assert!(!a.version.is_empty());
    }

    #[test]
    fn fnv1a_hex_differs_for_different_input() {
        assert_ne!(fnv1a_hex(b"a"), fnv1a_hex(b"b"));
        assert_eq!(fnv1a_hex(b"same"), fnv1a_hex(b"same"));
    }

    /// The platform-neutral loader (`kubuno_desktop_views_model::json`, WV-1) reads this export back whole:
    /// every component, property and event, with the same names, kinds and flags.
    #[test]
    fn the_model_loader_reads_the_real_export_back() {
        let exported = export();
        // One snapshot: other tests register project classes into the process-wide registry concurrently.
        let json = serde_json::to_string(&exported).expect("the export serializes");
        let doc = kubuno_desktop_views_model::load_registry(&json).expect("the export is a valid kbview-registry.json");
        assert_eq!(doc.version, exported.version);
        assert_eq!(doc.components.len(), exported.components.len());
        for (read, written) in doc.components.iter().zip(&exported.components) {
            assert_eq!(read.name, written.name);
            assert_eq!(read.children, written.children);
            assert_eq!(read.layout_kind, written.layout_kind);
            assert_eq!(read.base_chain, written.base_chain);
            assert_eq!(read.properties.len(), written.properties.len(), "{}", written.name);
            for (p, w) in read.properties.iter().zip(&written.properties) {
                assert_eq!((p.name.as_str(), p.inherited_from.as_deref(), p.root_only), (w.name, w.inherited_from, w.root_only));
                let kind = match &w.kind {
                    PropKindJson::Bool => kubuno_desktop_views_model::PropKindEntry::Bool,
                    PropKindJson::F32 => kubuno_desktop_views_model::PropKindEntry::F32,
                    PropKindJson::String => kubuno_desktop_views_model::PropKindEntry::String,
                    PropKindJson::Enum(v) => kubuno_desktop_views_model::PropKindEntry::Enum(v.iter().map(|s| s.to_string()).collect()),
                };
                assert_eq!(p.kind, kind, "{}.{}", written.name, w.name);
            }
            let events: Vec<&str> = read.events.iter().map(|e| e.name.as_str()).collect();
            let written_events: Vec<&str> = written.events.iter().map(|e| e.name).collect();
            assert_eq!(events, written_events, "{}", written.name);
        }
    }
}
