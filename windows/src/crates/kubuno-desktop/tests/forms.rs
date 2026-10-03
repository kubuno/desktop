//! Forms and controls built in code: collections, names, geometry, events, dialog results.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop::prelude::*;
use kubuno_desktop::views::binding::ViewModel;
use kubuno_desktop::views::events::ElementRef;
use kubuno_desktop::views::runtime::Runtime;

fn sender<'a>(name: Option<&'a str>, element: &'static str) -> ElementRef<'a> {
    ElementRef { name, element, id: "0", bounds: Default::default(), focus_id: None, attributes: &[] }
}

/// The attribute `attr` of the element named `name` in `text`.
fn attribute(text: &str, name: &str, attr: &str) -> Option<String> {
    let start = text.find(&format!("x:Name=\"{name}\""))?;
    let line_start = text[..start].rfind('<')?;
    let element = &text[line_start..];
    let element = &element[..element.find('>')?];
    let at = element.find(&format!(" {attr}=\""))? + attr.len() + 3;
    element[at..].split('"').next().map(str::to_string)
}

#[test]
fn controls_are_added_named_and_removed() {
    let form = Form::new().text("Code first").client_size(360.0, 140.0);
    let name = TextField::new().location(16.0, 16.0).size(328.0, 36.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    let ok = Button::new().text("OK").location(264.0, 84.0).size(80.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
    let cancel = Button::new().name("cancel").text("Cancel");
    form.controls().add(&name);
    form.controls().add(&ok);
    form.controls().add(&cancel);
    form.controls().add(&ok); // twice: once
    assert_eq!(form.controls().len(), 3);
    assert_eq!((name.get_name(), ok.get_name(), cancel.get_name()), ("textField1".into(), "button1".into(), "cancel".into()));
    assert_eq!(form.control("button1"), Some(ok.as_control().clone()));

    let text = kubuno_desktop::__private::compose_text(&form);
    assert_eq!(attribute(&text, "button1", "Anchor").as_deref(), Some("Bottom, Right"));
    assert_eq!(attribute(&text, "button1", "X").as_deref(), Some("264"));
    assert_eq!(attribute(&text, "textField1", "Width").as_deref(), Some("328"));
    assert!(text.starts_with("<Panel"), "{text}");
    assert!(text.contains("DesignWidth=\"360\""), "{text}");
    let mut runtime = Runtime::new();
    assert!(runtime.reload_from_text(&text), "{text}\n{:?}", runtime.diagnostics());
    assert_eq!(runtime.design_size(), Some((360.0, 140.0)));

    assert!(form.controls().remove(&cancel));
    assert!(!form.controls().remove(&cancel));
    let text = kubuno_desktop::__private::compose_text(&form);
    assert!(!text.contains("x:Name=\"cancel\""), "{text}");
    assert!(cancel.form().is_none());
    form.controls().clear();
    assert!(form.controls().is_empty());
    assert!(!kubuno_desktop::__private::compose_text(&form).contains("button1"));
}

#[test]
fn containers_hold_their_own_controls() {
    let form = Form::new();
    let group = GroupBox::new().text("Options").bounds(8.0, 8.0, 300.0, 120.0);
    let check = CheckBox::new().text("Remember me").checked(true).location(12.0, 32.0);
    group.controls().add(&check);
    form.controls().add(&group);
    assert_eq!(check.get_name(), "checkBox1", "named when its container joined the form");
    assert_eq!(form.controls().find("checkBox1"), Some(check.as_control().clone()));
    let text = kubuno_desktop::__private::compose_text(&form);
    let group_at = text.find("x:Name=\"groupBox1\"").expect("group");
    let check_at = text.find("x:Name=\"checkBox1\"").expect("check");
    assert!(check_at > group_at && text[group_at..check_at].contains('>'), "{text}");
    assert!(kubuno_desktop::views::compile::compile_in(&text, None).is_ok(), "{text}");
    assert!(check.is_checked());
}

#[test]
fn bound_properties_change_without_recomposition_and_others_recompose() {
    let mut form = Form::new();
    let label = Label::new().text("before");
    form.controls().add(&label);
    let text = kubuno_desktop::__private::compose_text(&form);
    assert!(!form.root().get_name().contains("panel"), "the root has no name of its own");
    let path = attribute(&text, "label1", "Text").and_then(|b| b.strip_prefix("{Binding ").and_then(|b| b.split(',').next()).map(str::to_string)).expect("bound");
    label.set_text("after");
    assert_eq!(form.get(&path), Some(Value::Str("after".into())));
    // `Role` is not bound: it takes a new composition.
    label.set_property("Role", "Heading");
    let text = kubuno_desktop::__private::compose_text(&form);
    assert_eq!(attribute(&text, "label1", "Role").as_deref(), Some("Heading"));
    // The user edits a bound value: the handle sees it.
    form.set(&path, Value::Str("typed".into()));
    assert_eq!(label.get_text(), "typed");
}

#[test]
fn subscribers_receive_typed_senders_in_order_and_can_unsubscribe() {
    let mut form = Form::new();
    let ok = Button::new().text("OK");
    form.controls().add(&ok);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let first = ok.click().subscribe(move |sender: &Button, e| log.borrow_mut().push(format!("1 {} {}", sender.get_text(), e.clicks)));
    let log = seen.clone();
    ok.click().subscribe(move |_, _| log.borrow_mut().push("2".to_string()));
    let log = seen.clone();
    form.load().subscribe(move |sender: &Form, _| log.borrow_mut().push(format!("load {}", sender.controls().len())));
    let text = kubuno_desktop::__private::compose_text(&form);
    let click = attribute(&text, "button1", "OnClick").expect("routed");
    form.dispatch_event(&click, &sender(Some("button1"), "Button"), &mut MouseEventArgs { clicks: 1, ..Default::default() });
    let load = text.split("OnLoad=\"").nth(1).and_then(|t| t.split('"').next()).expect("root load").to_string();
    form.dispatch_event(&load, &sender(None, "Panel"), &mut EventArgs);
    assert_eq!(*seen.borrow(), ["1 OK 1", "2", "load 1"]);
    assert!(ok.click().unsubscribe(first));
    form.dispatch_event(&click, &sender(Some("button1"), "Button"), &mut MouseEventArgs::default());
    assert_eq!(seen.borrow().last().map(String::as_str), Some("2"));
    assert_eq!(seen.borrow().len(), 4);
}

#[test]
fn a_button_dialog_result_closes_its_modal_form() {
    let mut form = MessageBox::build("Delete the file?", "Files", MessageBoxButtons::YesNo, MessageBoxIcon::Question);
    let text = kubuno_desktop::__private::compose_text(&form);
    assert!(kubuno_desktop::views::compile::compile_in(&text, None).is_ok(), "{text}");
    assert_eq!(form.get_text(), "Files");
    let no = form.control("button2").expect("No button");
    assert_eq!(no.get_text(), "No");
    // The web dialogs' footer: text buttons in the window's action bar, on its right.
    assert_eq!(attribute(&text, "button1", "Variant").as_deref(), Some("Text"));
    assert_eq!(attribute(&text, "button1", "ActionBar.Region").as_deref(), Some("Right"));
    assert!(text.contains("WindowKind=\"Dialog\""), "{text}");

    // Not modal: the result is recorded, nothing closes.
    let click = attribute(&text, "button2", "OnClick").expect("routed");
    form.dispatch_event(&click, &sender(Some("button2"), "Button"), &mut MouseEventArgs::default());
    assert_eq!(form.dialog_result(), DialogResult::No);
    assert!(!form.is_modal());

    // Setting a result on a modal form asks it to close (Windows Forms).
    kubuno_desktop::__private::set_modal(&form, true);
    form.set_dialog_result(DialogResult::Yes);
    assert!(kubuno_desktop::__private::close_requested(&form));
}

#[test]
fn anchors_and_docks_read_and_write_the_kbview_spelling() {
    assert_eq!((Anchor::TOP | Anchor::RIGHT).to_xml(), "Top, Right");
    assert_eq!(Anchor::NONE.to_xml(), "None");
    assert_eq!(Anchor::parse("Bottom, Left"), Anchor::BOTTOM | Anchor::LEFT);
    assert_eq!(Anchor::default(), Anchor::TOP | Anchor::LEFT);
    let form = Form::new();
    let panel = Panel::new().dock(DockStyle::Fill);
    form.controls().add(&panel);
    let text = kubuno_desktop::__private::compose_text(&form);
    assert_eq!(attribute(&text, "panel1", "Dock").as_deref(), Some("Fill"));
}
