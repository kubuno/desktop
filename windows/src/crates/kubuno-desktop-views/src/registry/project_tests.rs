//! The registry's application tier (`registry::project`). Classes derived inside `kubuno_desktop_views`
//! are not registered by their derive (the built-ins must not be), so these tests register by
//! hand what the derive of an application would (the integration test `tests/custom_controls.rs`
//! covers the derive's own registration). Each test uses its own class names: the registry is
//! global to the test process.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::binding::{HandlerTable, MapViewModel, Value, ViewModel};
use crate::component::{Component, ComponentLink, Control, HasComponentCore};
use crate::controls::Button;
use crate::events::{Event, MouseEventArgs};
use crate::registry::{self, lookup, EventCategory, EventMeta, PropKind, PropertyMeta};

#[derive(crate::component::Component, Default)]
#[kubuno(extends = Button, overrides(Control))]
#[category("Kubuno")]
struct TestRound {
    base: Button,
    #[property]
    #[category("Appearance")]
    #[default_value(12.0)]
    corner_radius: f32,
    #[property]
    caption: String,
    #[event]
    #[category("Mouse")]
    long_press: Event<MouseEventArgs>,
}

impl Control for TestRound {}

static ROUND_PROPS: [PropertyMeta; 2] = [
    PropertyMeta::new("CornerRadius", PropKind::F32, "12.0", "The radius.").category("Appearance"),
    PropertyMeta::new("Caption", PropKind::String, "", ""),
];
static ROUND_EVENTS: [EventMeta; 1] = [EventMeta::new("OnLongPress", "Held.").category(EventCategory::Mouse).args::<MouseEventArgs>()];

fn create_round() -> Option<Rc<RefCell<dyn Component>>> {
    Some(Rc::new(RefCell::new(TestRound::default())))
}

static ROUND: ClassRegistration = ClassRegistration {
    name: "TestRound",
    crate_name: "test_app",
    kind: ClassKind::Control,
    doc: "A round button.",
    extends: "Button",
    chain: <TestRound as crate::component::Lineage>::CHAIN,
    create: create_round,
    properties: &ROUND_PROPS,
    events: &ROUND_EVENTS,
    default_event: None,
    default_property: Some("CornerRadius"),
    toolbox_category: Some("Kubuno"),
    toolbox_icon: Some("circle"),
    browsable: true,
    view: None,
    view_path: None,
    view_dir: None,
    source_file: "round.rs",
};

static CLASH: ClassRegistration = ClassRegistration {
    name: "Button",
    crate_name: "evil",
    kind: ClassKind::Control,
    doc: "Not the button.",
    extends: "Button",
    chain: <TestRound as crate::component::Lineage>::CHAIN,
    create: create_round,
    properties: &ROUND_PROPS,
    events: &ROUND_EVENTS,
    default_event: None,
    default_property: None,
    toolbox_category: None,
    toolbox_icon: None,
    browsable: true,
    view: None,
    view_path: None,
    view_dir: None,
    source_file: "evil.rs",
};

fn root_of(text: &str) -> crate::ast::Element {
    use crate::ast::AstNode;
    let parse = crate::syntax::parse(text);
    crate::ast::Document::cast(parse.syntax()).and_then(|d| d.root_element()).expect("a root element")
}

#[test]
fn a_linked_class_inherits_its_base_and_adds_its_own_members() {
    register_class(&ROUND);
    register_class(&ROUND); // twice: harmless
    register_class(&CLASH);
    let meta = lookup("TestRound").expect("registered");
    assert_eq!(meta.base_chain(), ["TestRound", "Button", "ButtonBase", "Control", "Component"]);
    let props: Vec<&str> = meta.properties.iter().map(|p| p.name).collect();
    assert_eq!(&props[..2], ["CornerRadius", "Caption"], "own properties first");
    assert!(props.contains(&"Text") && props.contains(&"Variant"), "the base's properties are inherited: {props:?}");
    assert_eq!(meta.property("CornerRadius").and_then(|p| p.category), Some("Appearance"));
    assert!(meta.event("OnLongPress").is_some() && meta.event("OnClick").is_some() && meta.event("OnMouseDown").is_some());
    assert_eq!(meta.default_event(), Some("OnClick"), "the base's default event");
    let builtin_doc = registry::builtins().iter().find(|m| m.name == "Button").map(|m| m.doc);
    assert_eq!(lookup("Button").map(|m| m.doc), builtin_doc, "a class named like a built-in is refused");
    let info = project_info("TestRound").expect("info");
    assert_eq!((info.origin, info.kind, info.toolbox_icon, info.crate_name), (Origin::Linked, ClassKind::Control, Some("circle"), Some("test_app")));
    assert!(crate::controls::class_of("TestRound").is_some_and(|c| (c.create)().is_some()));

    // A view using it validates and compiles (its node is `<Button>`'s).
    let view = r#"<Panel><TestRound x:Name="r" Text="Ok" CornerRadius="18" Caption="{Binding Title}" OnLongPress="held"/></Panel>"#;
    assert!(crate::validate::validate_with_default_registry(&crate::syntax::parse(view)).is_empty());
    assert!(crate::compile::compile(view).is_ok());
    let bad = crate::validate::validate_with_default_registry(&crate::syntax::parse(r#"<TestRound Radius="1"/>"#));
    assert!(bad.iter().any(|d| d.message.contains("unknown attribute `Radius`")), "{bad:?}");

    // The export marks it.
    let json = crate::registry::export::components_json();
    let entry = json.iter().find(|c| c.name == "TestRound").expect("exported");
    assert_eq!((entry.origin, entry.kind, entry.family, entry.linked), ("project", "control", "project", true));
    assert_eq!(entry.icon, "circle");
    assert!(entry.properties.iter().any(|p| p.name == "CornerRadius" && p.category == Some("Appearance")));
}

#[test]
fn the_element_s_own_properties_are_set_on_the_instance() {
    let element = root_of(r#"<TestRound CornerRadius="18" Caption="{Binding Title}"/>"#);
    let props: Vec<crate::design::CustomProp> = ROUND_PROPS.iter().filter_map(|p| crate::design::CustomProp::read(&element, p)).collect();
    assert_eq!(props.len(), 2);
    let mut vm = MapViewModel::new();
    vm.set("Title", Value::Str("Hello".into()));
    let mut round = TestRound::default();
    crate::design::apply_custom_props(&mut round, &props, &vm);
    assert_eq!((round.corner_radius, round.caption.as_str()), (18.0, "Hello"));
    assert_eq!(round.kubuno_get_property("CornerRadius"), Some(Value::F32(18.0)));
    assert!(!round.kubuno_set_property("Nope", &Value::Bool(true)));
}

#[test]
fn a_declared_event_reaches_the_rust_subscribers_then_the_element_s_handler() {
    use crate::events::router::SlotEvents;
    register_class(&ROUND);
    let element = root_of(r#"<TestRound x:Name="r" OnLongPress="held"/>"#);
    let slot = SlotEvents::from_element(&element, lookup("TestRound").expect("registered"), false);
    assert_eq!(slot.handler("OnLongPress"), Some("held"));

    let mut round = TestRound::default();
    let seen = Rc::new(std::cell::Cell::new(0u8));
    let s = seen.clone();
    round
        .long_press
        .subscribe(move |sender, e: &mut MouseEventArgs| {
            assert_eq!(sender.element, "TestRound");
            e.clicks = 7;
            s.set(1);
        })
        .detach();
    round.raise_long_press(MouseEventArgs::default());
    assert_eq!(seen.get(), 1, "Rust subscribers run at once");

    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    let mut handlers = HandlerTable::new();
    handlers.insert("held", Box::new(move |_vm, v| l.borrow_mut().push(format!("held {v:?}"))));
    let mut vm = MapViewModel::new();
    let mut events = Vec::new();
    crate::design::deliver_queued(&mut round, Some(&slot), kubuno_desktop_ui::Rect::default(), &mut handlers, &mut vm, &mut events);
    assert_eq!(*log.borrow(), ["held Bool(true)"]);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        crate::node::ViewEventKind::Other { name, args } => {
            assert_eq!(*name, "LongPress");
            assert_eq!(args.downcast_ref::<MouseEventArgs>().map(|a| a.clicks), Some(7), "the args the subscribers wrote");
        }
        other => panic!("unexpected {other:?}"),
    }

    // In the designer the queue is emptied, nothing runs.
    round.raise_long_press(MouseEventArgs::default());
    crate::design::deliver_queued(&mut round, None, kubuno_desktop_ui::Rect::default(), &mut handlers, &mut vm, &mut events);
    assert_eq!(log.borrow().len(), 1);
    assert!(!round.component_core().has_queued());
}

#[test]
fn declared_classes_are_placeholders_that_never_shadow_linked_ones() {
    set_declared(vec![
        DeclaredComponent {
            name: "TestGaugeDeclared".into(),
            extends: "Control".into(),
            base_chain: vec!["TestGaugeDeclared".into(), "Control".into(), "Component".into()],
            properties: vec![DeclaredProperty { name: "Level".into(), kind: DeclaredKind::F32, default: "0".into(), category: Some("Data".into()), ..Default::default() }],
            events: vec![
                DeclaredEvent { name: "OnFull".into(), category: "Behavior".into(), args_type: "CancelEventArgs".into(), ..Default::default() },
                DeclaredEvent { name: "OnMouseDown".into(), inherited_from: Some("Control".into()), ..Default::default() },
            ],
            ..Default::default()
        },
        DeclaredComponent {
            name: "TestTickerDeclared".into(),
            kind: "component".into(),
            extends: "Component".into(),
            base_chain: vec!["TestTickerDeclared".into(), "Component".into()],
            ..Default::default()
        },
        DeclaredComponent { name: "TestRound".into(), extends: "Button".into(), base_chain: vec!["TestRound".into(), "Button".into()], ..Default::default() },
    ]);
    register_class(&ROUND);
    let gauge = lookup("TestGaugeDeclared").expect("declared");
    assert_eq!(gauge.base_chain(), ["TestGaugeDeclared", "Control", "Component"]);
    assert_eq!(gauge.property("Level").map(|p| (p.kind, p.category)), Some((PropKind::F32, Some("Data"))));
    let full = gauge.event("OnFull").expect("own event");
    assert!(full.cancelable && full.args_rust == "CancelEventArgs" && full.category == EventCategory::Behavior);
    assert_eq!(gauge.events.len(), 1, "inherited events of an export entry are not re-declared");
    assert!(gauge.event("OnMouseDown").is_some(), "they come from the chain");
    assert!(crate::compile::compile(r#"<Panel><TestGaugeDeclared Level="3"/><TestTickerDeclared/></Panel>"#).is_ok());
    assert!(registry::is_non_visual("TestTickerDeclared") && !registry::is_non_visual("TestGaugeDeclared"));
    assert_eq!(project_info("TestGaugeDeclared").map(|i| i.origin), Some(Origin::Declared));
    assert_eq!(project_info("TestRound").map(|i| i.origin), Some(Origin::Linked), "the linked class wins");
    assert!(crate::controls::class_of("TestGaugeDeclared").is_some_and(|c| (c.create)().is_none()));

    // An export entry reads back as a declared class (what Visual Studio sends the surface).
    let entry = crate::registry::export::components_json().into_iter().find(|c| c.name == "TestGaugeDeclared").expect("exported");
    let back: DeclaredComponent = serde_json::from_value(serde_json::to_value(entry).expect("json")).expect("reads back");
    assert_eq!((back.name.as_str(), back.kind.as_str(), back.extends.as_str()), ("TestGaugeDeclared", "control", "Control"));
    assert!(back.properties.iter().any(|p| p.name == "Level" && p.kind == DeclaredKind::F32));
}
