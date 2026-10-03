//! `#[kubuno_desktop::view]` form classes: generated members, `initialize_component`, handlers, bindings.

use kubuno_desktop::prelude::*;
use kubuno_desktop::views::binding::ViewModel;
use kubuno_desktop::views::events::ElementRef;
use kubuno_desktop::views::runtime::Runtime;

#[kubuno_desktop::view(xml = r#"
<Panel DesignWidth="400" DesignHeight="120" Title="Greeter" OnLoad="main_view_load">
  <TextField x:Name="status" Text="Ready." X="16" Y="16" Width="260" Height="36" Anchor="Top, Left, Right"/>
  <Button x:Name="hello" Text="Say hello" OnClick="hello_click" X="288" Y="16" Width="96" Height="36" Anchor="Top, Right"/>
  <Switch x:Name="loud" On="false" OnCheckedChanged="loud_changed" X="16" Y="64" Width="120" Height="32"/>
  <Label Text="unnamed" X="16" Y="100" Width="100" Height="20"/>
  <Button Text="anonymous" OnClick="any_click" X="200" Y="64" Width="96" Height="32"/>
</Panel>"#)]
pub struct Greeter {
    clicks: u32,
    loaded: bool,
    log: Vec<String>,
    #[bind]
    user_name: String,
}

impl Greeter {
    pub fn new() -> Self {
        let mut view = Self::default();
        view.initialize_component();
        view
    }

    fn main_view_load(&mut self, _sender: &Form, _e: &EventArgs) {
        self.loaded = true;
    }

    fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs) {
        self.clicks += 1;
        self.log.push(format!("{} at {}", sender.get_text(), e.x));
        self.status.set_text(format!("Hello {}", self.clicks));
        self.hello.set_enabled(self.clicks < 2);
    }

    fn loud_changed(&mut self, e: &CheckedChangedEventArgs) {
        self.log.push(format!("loud={}", e.new));
    }

    fn any_click(&mut self) {
        self.log.push("any".to_string());
    }
}

fn sender<'a>(name: Option<&'a str>, element: &'static str) -> ElementRef<'a> {
    ElementRef { name, element, id: "0", bounds: Default::default(), focus_id: None, attributes: &[] }
}

/// The synthetic handler the composed view gives `control`'s `event`.
fn handler_of(text: &str, control: &str, event: &str) -> String {
    let start = text.find(&format!("x:Name=\"{control}\"")).expect("control in the composed view");
    let tail = &text[start..];
    let end = tail.find("/>").unwrap_or(tail.len());
    let element = &tail[..end];
    let at = element.find(&format!("{event}=\"")).expect("event attribute") + event.len() + 2;
    element[at..].split('"').next().unwrap_or_default().to_string()
}

fn composed(view: &Greeter) -> String {
    kubuno_desktop::__private::compose_text(view.form())
}

#[test]
fn initialize_component_links_the_fields_to_the_view() {
    let view = Greeter::new();
    assert_eq!(view.status.get_text(), "Ready.");
    assert_eq!(view.hello.get_text(), "Say hello");
    assert_eq!(view.hello.get_name(), "hello");
    assert_eq!(view.hello.element(), "Button");
    assert!(!view.loud.is_checked());
    assert_eq!(view.hello.get_location(), (288.0, 16.0));
    assert_eq!(view.hello.get_anchor(), Anchor::TOP | Anchor::RIGHT);
    // `Deref` to the form.
    assert_eq!(view.get_text(), "Greeter");
    assert_eq!(view.get_client_size(), (400.0, 120.0));
    assert_eq!(<Greeter as View>::HANDLERS, &["main_view_load", "hello_click", "loud_changed", "any_click"]);
}

#[test]
fn the_composed_view_binds_the_named_controls_and_compiles() {
    let view = Greeter::new();
    let text = composed(&view);
    assert!(text.contains("Text=\"{Binding __kb."), "{text}");
    assert!(!text.contains("hello_click"), "handlers are routed through the form: {text}");
    assert!(text.contains("Text=\"unnamed\""), "an unnamed element keeps its literals: {text}");
    assert!(kubuno_desktop::views::compile::compile_in(&text, None).is_ok(), "{text}");
    let mut runtime = Runtime::new();
    assert!(runtime.reload_from_text(&text));
    assert_eq!(runtime.design_size(), Some((400.0, 120.0)));
}

#[test]
fn events_run_the_methods_the_view_names_with_typed_senders_and_args() {
    let mut view = Greeter::new();
    let text = composed(&view);
    let click = handler_of(&text, "hello", "OnClick");
    let mut e = MouseEventArgs { x: 7.0, ..Default::default() };
    assert!(view.dispatch_event(&click, &sender(Some("hello"), "Button"), &mut e));
    assert_eq!(view.clicks, 1);
    assert_eq!(view.log, ["Say hello at 7"]);
    // What the handler set shows through the bindings of the composed view.
    let status_text = text.split("x:Name=\"status\"").nth(1).and_then(|t| t.split("Text=\"{Binding ").nth(1)).and_then(|t| t.split(',').next()).expect("status bound");
    assert_eq!(view.get(status_text), Some(Value::Str("Hello 1".into())));
    assert!(view.hello.is_enabled());
    view.dispatch_event(&click, &sender(Some("hello"), "Button"), &mut MouseEventArgs::default());
    assert!(!view.hello.is_enabled());

    // A handler taking only the args, and one taking nothing, on an unnamed element.
    let toggled = handler_of(&text, "loud", "OnCheckedChanged");
    let mut args = CheckedChangedEventArgs { new: true, ..Default::default() };
    view.dispatch_event(&toggled, &sender(Some("loud"), "Switch"), &mut args);
    assert_eq!(view.log.last().map(String::as_str), Some("loud=true"));

    let load = text.split("OnLoad=\"").nth(1).and_then(|t| t.split('"').next()).expect("root load").to_string();
    view.dispatch_event(&load, &sender(None, "Panel"), &mut EventArgs);
    assert!(view.loaded);
}

#[test]
fn typing_in_a_bound_control_updates_its_handle() {
    let mut view = Greeter::new();
    let text = composed(&view);
    let path = text.split("x:Name=\"status\"").nth(1).and_then(|t| t.split("Text=\"{Binding ").nth(1)).and_then(|t| t.split(',').next()).expect("bound").to_string();
    view.set(&path, Value::Str("typed".into()));
    assert_eq!(view.status.get_text(), "typed");
}

#[test]
fn rust_subscribers_run_after_the_views_handler() {
    let mut view = Greeter::new();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    view.hello.click().subscribe(move |sender, e| log.borrow_mut().push(format!("{}:{}", sender.get_name(), e.clicks)));
    let log = seen.clone();
    // An event the view does not name: subscribing routes it (after a recomposition).
    view.status.on::<EventArgs>("OnGotFocus").subscribe(move |sender, _| log.borrow_mut().push(format!("focus {}", sender.get_name())));
    let text = composed(&view);
    let click = handler_of(&text, "hello", "OnClick");
    view.dispatch_event(&click, &sender(Some("hello"), "Button"), &mut MouseEventArgs { clicks: 1, ..Default::default() });
    assert_eq!(view.clicks, 1, "the view's handler ran");
    let focus = handler_of(&text, "status", "OnGotFocus");
    view.dispatch_event(&focus, &sender(Some("status"), "TextField"), &mut EventArgs);
    assert_eq!(*seen.borrow(), ["hello:1", "focus status"]);
}

#[test]
fn bind_fields_answer_their_binding_path() {
    let mut view = Greeter::new();
    view.user_name = "Ada".into();
    assert_eq!(view.get("UserName"), Some(Value::Str("Ada".into())));
    view.set("UserName", Value::Str("Grace".into()));
    assert_eq!(view.user_name, "Grace");
}

#[test]
fn code_created_controls_join_a_designed_view() {
    let view = Greeter::new();
    let extra = Button::new().text("Extra").bounds(16.0, 90.0, 80.0, 24.0);
    view.controls().add(&extra);
    assert_eq!(extra.get_name(), "button1");
    let text = composed(&view);
    assert!(text.contains("x:Name=\"button1\""), "{text}");
    assert!(kubuno_desktop::views::compile::compile_in(&text, None).is_ok(), "{text}");
    assert_eq!(view.control("button1"), Some(extra.as_control().clone()));
}

// A view read from a file, relative to this source file.
#[kubuno_desktop::view("fixtures/settings_view.kbview")]
#[derive(Default)]
struct SettingsView;

#[test]
fn a_view_file_is_read_and_embedded() {
    let mut view = SettingsView::default();
    view.initialize_component();
    assert_eq!(view.ok.get_text(), "OK");
    assert_eq!(view.name.get_text(), "");
    view.name.set_text("Ada");
    assert_eq!(view.name.get_text(), "Ada");
}
