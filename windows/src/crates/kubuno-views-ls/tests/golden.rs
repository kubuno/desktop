//! EVT-7b's shared golden test (`vskubuno/docs/EVENTS.md` §7): the metadata the derive macro
//! compiles into a control's registration (read back from the registry export, as Visual Studio
//! reads it) equals what the language server's `syn` scan of the same source produces — both go
//! through `kubuno_views_meta`, and this checks the two ends agree field by field.

use kubuno_views::prelude::*;
use kubuno_views::registry::{self, DeclaredComponent};

/// Emits the items and keeps their source text for the scan.
macro_rules! golden {
    ($($item:item)*) => {
        $($item)*
        const SOURCE: &str = stringify!($($item)*);
    };
}

golden! {
    /// A button drawn as a pill.
    #[derive(Component, Default)]
    #[kubuno(extends = Button, overrides(Control))]
    #[category("Kubuno")]
    #[toolbox(icon = "circle")]
    #[default_event("LongPress")]
    #[default_property("CornerRadius")]
    pub struct GoldenRound {
        base: Button,
        /// The radius of the corners, in pixels.
        #[property]
        #[category("Appearance")]
        #[default_value(18.0)]
        pub corner_radius: f32,
        #[property(bindable)]
        #[description("The outline.")]
        #[default_value(GoldenShape::Square)]
        pub shape: GoldenShape,
        #[property]
        #[browsable(false)]
        pub secret: String,
        #[property(name = "Tint")]
        #[localizable]
        #[editor("color")]
        #[type_converter("ColorConverter")]
        #[designer_serialization_visibility(Hidden)]
        pub color: Option<String>,
        #[property]
        pub enabled_twice: bool,
        /// Occurs when the button is held.
        #[event]
        #[category("Mouse")]
        pub long_press: Event<MouseEventArgs>,
        #[event(name = "Closing")]
        #[category("Behavior")]
        #[browsable(false)]
        pub about_to_close: Event<CancelEventArgs>,
    }

    impl Control for GoldenRound {}

    /// The outline of a golden button.
    #[derive(PropertyValue, Default, Clone, Copy)]
    pub enum GoldenShape {
        #[default]
        Pill,
        Square,
    }

    /// Ticks.
    #[derive(Component, Default)]
    #[kubuno(extends = Component)]
    #[toolbox(hidden)]
    pub struct GoldenTicker {
        base: ComponentCore,
        #[property]
        #[default_value(2)]
        pub every: u32,
    }
}

fn linked(name: &str) -> DeclaredComponent {
    let entry = registry::export::components_json().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name} is registered"));
    serde_json::from_value(serde_json::to_value(entry).expect("json")).expect("an export entry reads as a declared class")
}

#[test]
fn the_macro_and_the_language_server_read_the_same_metadata() {
    let scan = kubuno_views_meta::scan_source(SOURCE);
    assert_eq!(scan.components.len(), 2);
    for decl in &scan.components {
        let scanned = kubuno_views_ls::project::to_declared(decl, &scan.components, Some("golden"), None, None);
        let linked = linked(&decl.name);

        assert_eq!(linked.name, scanned.name);
        assert_eq!(linked.kind, scanned.kind, "{}", decl.name);
        assert_eq!(linked.doc, scanned.doc, "{}", decl.name);
        assert_eq!(linked.crate_name, scanned.crate_name, "{}", decl.name);
        assert_eq!(linked.extends, scanned.extends, "{}", decl.name);
        assert_eq!(linked.base_chain, scanned.base_chain, "{}", decl.name);
        assert_eq!(linked.default_event, scanned.default_event, "{}", decl.name);
        assert_eq!(linked.default_property, scanned.default_property, "{}", decl.name);
        assert_eq!(linked.toolbox_category, scanned.toolbox_category, "{}", decl.name);
        assert_eq!(linked.toolbox_icon, scanned.toolbox_icon, "{}", decl.name);
        assert_eq!(linked.browsable, scanned.browsable, "{}", decl.name);

        // The export lists the class's own properties first, then the base's.
        assert!(linked.properties.len() >= scanned.properties.len());
        assert_eq!(linked.properties[..scanned.properties.len()], scanned.properties[..], "{}", decl.name);

        // Its own events first (then the base's own, then the inherited ones).
        let own: Vec<_> = linked.events.iter().filter(|e| e.inherited_from.is_none() && !e.root_only).take(scanned.events.len()).collect();
        assert_eq!(own.len(), scanned.events.len());
        for (l, s) in own.iter().zip(&scanned.events) {
            assert_eq!((&l.name, &l.doc, &l.category, &l.args_type, l.browsable), (&s.name, &s.doc, &s.category, &s.args_type, s.browsable), "{}", decl.name);
        }
    }
    let round = linked("GoldenRound");
    assert_eq!(round.properties[1].kind, registry::DeclaredKind::Enum(vec!["Pill".into(), "Square".into()]), "enums are resolved by the scan too");
    assert_eq!(round.properties[3].name, "Tint");
    assert!(!linked("GoldenTicker").browsable);
}
