//! A `<TextField PasswordChar="…">` bound two-way: what the user types reaches the view model as
//! typed (only the drawing is masked), and assistive technology sees a password field whose value
//! is the glyphs, never the secret. Found by the shell's sign-in page, whose password arrived at
//! the server as « •••• ».

use kubuno_desktop_controls::host::{self, access::AccessRole, Frame, InputEvent, Modifiers};
use kubuno_desktop_views::binding::{HandlerTable, MapViewModel, Value, ViewModel};
use kubuno_desktop_views::runtime::Runtime;

const VIEW: &str = r#"<Panel DesignWidth="300" DesignHeight="200">
  <TextField x:Name="password" Text="{Binding Password, Mode=TwoWay}" PasswordChar="•" AccessibleName="Mot de passe" X="10" Y="10" Width="280" Height="36"/>
  <TextField x:Name="login" Text="{Binding Login, Mode=TwoWay}" AccessibleName="Identifiant" X="10" Y="60" Width="280" Height="36"/>
</Panel>"#;

fn frame_at(mouse: Option<(f32, f32)>, down: bool) -> Frame {
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
        wheel: (0.0, 0.0),
        click_count: 1,
        window_focused: true,
    }
}

fn run(rt: &mut Runtime, vm: &mut MapViewModel, f: Frame, input: Vec<InputEvent>) {
    host::input::set_frame_events(input);
    let mut handlers = HandlerTable::new();
    let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
    rt.frame(&canvas, &f, vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    host::input::set_frame_events(Vec::new());
}

/// Clicks at `(x, y)` (press, release), then types `text`.
fn type_at(rt: &mut Runtime, vm: &mut MapViewModel, x: f32, y: f32, text: &str) {
    run(rt, vm, frame_at(Some((x, y)), true), vec![]);
    run(rt, vm, frame_at(Some((x, y)), false), vec![]);
    run(rt, vm, frame_at(Some((x, y)), false), vec![InputEvent::Text(text.into())]);
    run(rt, vm, frame_at(Some((x, y)), false), vec![]);
}

fn text_of(vm: &MapViewModel, path: &str) -> Option<String> {
    match vm.get(path) {
        Some(Value::Str(s)) => Some(s),
        _ => None,
    }
}

#[test]
fn typing_into_a_password_field_binds_the_real_text() {
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    let mut vm = MapViewModel::new().with("Password", Value::Str(String::new())).with("Login", Value::Str(String::new()));
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);

    type_at(&mut rt, &mut vm, 100.0, 28.0, "s3cr\u{e9}t#]1");
    assert_eq!(text_of(&vm, "Password").as_deref(), Some("s3cr\u{e9}t#]1"), "the view model gets the typed password, not its glyphs");

    // Leaving the field (the binding is read back while it is not focused) keeps the real text.
    type_at(&mut rt, &mut vm, 100.0, 78.0, "camille");
    assert_eq!(text_of(&vm, "Login").as_deref(), Some("camille"));
    assert_eq!(text_of(&vm, "Password").as_deref(), Some("s3cr\u{e9}t#]1"), "unchanged once the field lost the focus");

    // Code setting the bound value reaches the (unfocused) field, and typing appends to it.
    vm.set("Password", Value::Str("abc".into()));
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    type_at(&mut rt, &mut vm, 280.0, 28.0, "d");
    let typed = text_of(&vm, "Password").unwrap_or_default();
    assert!(!typed.contains('\u{2022}'), "never a glyph in the model: {typed:?}");
    assert_eq!(typed.chars().count(), 4, "{typed:?}");
}

#[test]
fn assistive_technology_sees_a_password_field_without_its_text() {
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    let mut vm = MapViewModel::new().with("Password", Value::Str("hunter2".into())).with("Login", Value::Str("camille".into()));
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    let tree = host::access::last_published().expect("the frame publishes its accessibility tree");
    let password = tree.nodes.iter().find(|n| n.name == "Mot de passe").expect("the password field is published");
    assert_eq!(password.role, AccessRole::PasswordInput, "UI Automation reports IsPassword");
    assert_eq!(password.value.as_deref(), Some("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"), "the glyphs, never the secret");
    let login = tree.nodes.iter().find(|n| n.name == "Identifiant").expect("the login field is published");
    assert_eq!(login.role, AccessRole::TextInput);
    assert_eq!(login.value.as_deref(), Some("camille"), "a plain field still exposes its text");
}
