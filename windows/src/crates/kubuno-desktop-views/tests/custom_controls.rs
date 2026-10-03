//! EVT-7b of `vskubuno/docs/EVENTS.md`: an application's own controls, compiled in with
//! `#[derive(Component)]` / `#[derive(UserControl)]`, are registered by the derive itself (a
//! static constructor, before `main`) and usable as XML elements — this test crate is such an
//! application (`kubuno_desktop_views`' own classes are never registered).

use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::registry::{self, ClassKind, Origin};

/// A button drawn as a pill.
#[derive(Component, Default)]
#[kubuno(extends = Button, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "circle")]
#[default_property("CornerRadius")]
pub struct RoundButton {
    base: Button,
    /// The radius of the corners, in pixels.
    #[property]
    #[category("Appearance")]
    #[default_value(18.0)]
    pub corner_radius: f32,
    #[property(bindable)]
    #[description("The outline.")]
    pub shape: Shape,
    /// Occurs when the button is held.
    #[event]
    #[category("Mouse")]
    pub long_press: Event<MouseEventArgs>,
}

impl Control for RoundButton {
    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let r = e.clip_rectangle;
        let theme = e.graphics.theme();
        e.graphics.fill_rounded(&r, self.corner_radius, &theme.accent);
        e.raise(self, "OnPaint");
    }
}

/// The outline of a [`RoundButton`].
#[derive(PropertyValue, Default, Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    #[default]
    Pill,
    Square,
}

/// A gauge written from the `Control` level.
#[derive(Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
pub struct Gauge {
    base: ControlCore,
    #[property]
    pub level: f32,
}

impl Control for Gauge {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 120.0, height: 12.0 }
    }
}

/// A non-visual component.
#[derive(Component, Default)]
#[kubuno(extends = Component)]
pub struct Ticker {
    base: ComponentCore,
    #[property]
    pub every: u32,
}

/// A row of stars.
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/rating_bar.kbcontrol", default_event = "ValueCommitted")]
pub struct RatingBar {
    base: UserControlCore,
    #[property(bindable)]
    #[default_value(5)]
    pub max: u32,
    #[property(bindable)]
    pub value: u32,
    /// Occurs when the user picks a rating.
    #[event]
    pub value_committed: Event<EmptyEventArgs>,
}

#[kubuno_desktop_views::event_handlers]
impl RatingBar {
    fn star_click(&mut self) {
        self.value = (self.value + 1).min(self.max);
        self.raise_value_committed(EmptyEventArgs);
    }
}

#[test]
fn derived_classes_register_themselves_before_main() {
    for name in ["RoundButton", "Gauge", "Ticker", "RatingBar"] {
        let info = registry::project_info(name).unwrap_or_else(|| panic!("{name} is registered"));
        assert_eq!(info.origin, Origin::Linked);
        assert_eq!(info.crate_name, Some("custom_controls"));
    }
    assert_eq!(registry::project_info("Gauge").map(|i| i.kind), Some(ClassKind::Control));
    assert_eq!(registry::project_info("Ticker").map(|i| i.kind), Some(ClassKind::Component));
    assert_eq!(registry::project_info("RatingBar").map(|i| i.kind), Some(ClassKind::UserControl));
    assert!(registry::project_info("RatingBar").and_then(|i| i.view).is_some_and(|v| v.contains("x:Class=\"RatingBar\"")));

    let round = registry::lookup("RoundButton").expect("registered");
    assert_eq!(round.base_chain(), ["RoundButton", "Button", "ButtonBase", "Control", "Component"]);
    assert_eq!(round.property("Shape").map(|p| p.kind), Some(registry::PropKind::Enum(&["Pill", "Square"])));
    assert_eq!(round.property("CornerRadius").map(|p| (p.default, p.category, p.doc)), Some(("18.0", Some("Appearance"), "The radius of the corners, in pixels.")));
    assert!(round.property("Text").is_some(), "Button's properties are inherited");
    assert_eq!(round.event("OnLongPress").map(|e| (e.args_type, e.category)), Some(("MouseEventArgs", registry::EventCategory::Mouse)));
    assert_eq!(registry::lookup("RatingBar").and_then(|m| m.default_event()), Some("OnValueCommitted"));
    assert!(registry::is_non_visual("Ticker"));
}

/// A user control registers its view's folder (absolute): nested by a view of another folder, its own relative paths
/// (`d:ItemsSource="design/rows.json"`, images) still resolve next to its `.kbcontrol`.
#[test]
fn a_user_control_registers_the_folder_of_its_view() {
    let info = registry::project_info("RatingBar").expect("registered");
    let dir = std::path::Path::new(info.view_dir.expect("a view folder"));
    assert!(dir.is_absolute(), "{}", dir.display());
    assert_eq!(dir, std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures"));
    assert!(dir.join("rating_bar.kbcontrol").is_file());
    assert_eq!(registry::project_info("RoundButton").and_then(|i| i.view_dir), None, "a custom control has no view");
}

#[test]
fn views_use_them_as_elements() {
    let view = r#"
        <Panel DesignWidth="400" DesignHeight="200">
          <RoundButton x:Name="ok" Text="Ok" CornerRadius="12" Shape="Square" OnLongPress="held" X="8" Y="8" Width="100" Height="36"/>
          <Gauge Level="{Binding Level}" X="8" Y="60"/>
          <RatingBar x:Name="stars" Max="3" OnValueCommitted="rated" X="8" Y="100" Width="160" Height="36"/>
          <Ticker Every="2"/>
          <Timer Interval="500" Enabled="true" OnTick="tick"/>
        </Panel>"#;
    let diagnostics = kubuno_desktop_views::validate::validate_with_default_registry(&kubuno_desktop_views::syntax::parse(view));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(kubuno_desktop_views::compile::compile(view).is_ok());
    let bad = kubuno_desktop_views::validate::validate_with_default_registry(&kubuno_desktop_views::syntax::parse(r#"<RoundButton Shape="Round"/>"#));
    assert!(bad.iter().any(|d| d.message.contains("Round")), "an enum property is validated: {bad:?}");
}

#[test]
fn a_user_control_is_its_own_view_model_and_raises_its_events() {
    let mut r = RatingBar { max: 3, ..Default::default() };
    assert_eq!(r.get("Max"), Some(Value::F32(3.0)));
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let s = seen.clone();
    r.value_committed.subscribe(move |_, _| s.set(s.get() + 1)).detach();
    let sender = ElementRef::detached("star");
    assert!(ViewModel::dispatch_event(&mut r, "star_click", &sender, &mut EmptyEventArgs), "its #[event_handlers] run");
    assert_eq!((r.value, seen.get()), (1, 1));
    assert!(!ViewModel::dispatch_event(&mut r, "nope", &sender, &mut EmptyEventArgs));
    use kubuno_desktop_views::component::HasComponentCore;
    assert!(r.component_core().has_queued(), "queued for the element's handler");
}

#[test]
fn the_export_describes_them_for_the_tools() {
    let json = serde_json::to_value(registry::export::components_json()).expect("json");
    let find = |name: &str| json.as_array().and_then(|a| a.iter().find(|c| c["name"] == name)).cloned().unwrap_or_else(|| panic!("{name}"));
    let round = find("RoundButton");
    assert_eq!(round["origin"], "project");
    assert_eq!(round["linked"], true);
    assert_eq!(round["toolbox_icon"], "circle");
    assert_eq!(round["toolbox_category"], "Kubuno");
    assert_eq!(round["default_property"], "CornerRadius");
    assert_eq!(round["kind"], "control");
    assert_eq!(find("Ticker")["non_visual"], true);
    assert_eq!(find("RatingBar")["kind"], "user_control");
    assert_eq!(find("RatingBar")["view_path"], "fixtures/rating_bar.kbcontrol");
}

#[test]
fn a_custom_control_paints_frame_after_frame() {
    use kubuno_desktop_controls::host::{self, Frame, Modifiers};
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Gauge x:Name="g" X="0" Y="0" Width="100" Height="20"/></Panel>"#), "{:?}", rt.diagnostics());
    let frame = Frame {
        size: (300.0, 200.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 300.0, 200.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    for _ in 0..3 {
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &frame, &mut vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    }
}

/// A control with no override at all.
#[derive(Component, Default)]
#[kubuno(extends = Control)]
pub struct Plain {
    base: ControlCore,
}

#[test]
fn a_plain_custom_control_paints_frame_after_frame() {
    use kubuno_desktop_controls::host::{self, Frame, Modifiers};
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Plain x:Name="p" X="0" Y="0" Width="100" Height="20"/></Panel>"#), "{:?}", rt.diagnostics());
    let frame = Frame {
        size: (300.0, 200.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 300.0, 200.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    for _ in 0..3 {
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &frame, &mut vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    }
}

/// A frame of a 300×200 view with the pointer at `mouse` (away when `None`).
fn frame_at(mouse: Option<(f32, f32)>, down: bool) -> kubuno_desktop_controls::host::Frame {
    use kubuno_desktop_controls::host::{self, Frame, Modifiers};
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

#[test]
fn a_repeater_shows_one_user_control_per_row_each_its_own_instance() {
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(
        rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Repeater ItemsSource="{Binding Items}" ItemTemplate="RatingBar" ItemKey="Id" ItemHeight="40" X="0" Y="0" Width="300" Height="200"/></Panel>"#),
        "{:?}",
        rt.diagnostics()
    );
    let rows: Vec<kubuno_desktop_views::binding::Row> = (0..3).map(|i| kubuno_desktop_views::binding::Row::new().with("Id", Value::F32(i as f32)).with("Max", Value::F32(3.0))).collect();
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    let mut run = |rt: &mut kubuno_desktop_views::runtime::Runtime, vm: &mut kubuno_desktop_views::binding::MapViewModel, f| {
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &f, vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
        canvas.calls().into_iter().filter(|c| c.starts_with("text(")).collect::<Vec<_>>()
    };
    let texts = run(&mut rt, &mut vm, frame_at(None, false));
    assert_eq!(texts.iter().filter(|t| t.starts_with("text(\"0\"")).count(), 3, "three items, each its own RatingBar: {texts:?}");
    // A click on the first item's star runs that instance's handler only.
    run(&mut rt, &mut vm, frame_at(Some((80.0, 18.0)), true));
    run(&mut rt, &mut vm, frame_at(Some((80.0, 18.0)), false));
    let texts = run(&mut rt, &mut vm, frame_at(None, false));
    assert_eq!(texts.iter().filter(|t| t.starts_with("text(\"1\"")).count(), 1, "{texts:?}");
    assert_eq!(texts.iter().filter(|t| t.starts_with("text(\"0\"")).count(), 2, "{texts:?}");
}

#[test]
fn a_wrapping_stack_places_every_child() {
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(
        rt.reload_from_text(r#"<Panel DesignWidth="900" DesignHeight="600"><Tabs Dock="Fill" SelectedIndex="0"><TabItem Header="Cartes"><Stack Direction="TopDown" Gap="12" Padding="16"><Label Text="Titre" Role="Heading" Height="24"/><Stack Direction="LeftToRight" WrapContents="true" CrossAlign="Center" Gap="8" Height="72"><Badge Text="Rust"/><Badge Text="Kubuno"/></Stack><Stack Direction="LeftToRight" CrossAlign="Center" Gap="8" Height="40"><SearchField Placeholder="Rechercher" Stack.Fill="true"/><Button Text="Ajouter"/></Stack></Stack></TabItem></Tabs></Panel>"#),
        "{:?}",
        rt.diagnostics()
    );
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
    rt.frame(&canvas, &frame_at(None, false), &mut vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 900.0, 600.0));
    let texts: Vec<String> = canvas.calls().into_iter().filter(|c| c.starts_with("text(")).collect();
    assert!(texts.iter().any(|t| t.contains("Kubuno")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Ajouter")), "{texts:?}");
    // Below the title, in the canvas space of the stack (not at the window's top left).
    let top = |needle: &str| texts.iter().find(|t| t.contains(needle)).and_then(|t| t.rsplit(' ').next()).and_then(|r| r.split(',').nth(1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(-1.0);
    assert!(top("Kubuno") > top("Titre") && top("Ajouter") > top("Kubuno"), "{texts:?}");
}

/// A user control whose view names an `AcceptButton`.
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/confirm_box.kbcontrol")]
pub struct ConfirmBox {
    base: UserControlCore,
    #[property(bindable)]
    pub accepted: u32,
}

#[kubuno_desktop_views::event_handlers]
impl ConfirmBox {
    fn ok_click(&mut self) {
        self.accepted += 1;
    }
}

#[test]
fn enter_in_a_user_control_clicks_its_own_accept_button() {
    use kubuno_desktop_controls::host::{self, vk, InputEvent, Modifiers};
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><ConfirmBox x:Name="confirm" X="0" Y="0" Width="300" Height="40"/></Panel>"#), "{:?}", rt.diagnostics());
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    let mut run = |rt: &mut kubuno_desktop_views::runtime::Runtime, vm: &mut kubuno_desktop_views::binding::MapViewModel, f, keys: Vec<InputEvent>| {
        host::input::set_frame_events(keys);
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &f, vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
        host::input::set_frame_events(Vec::new());
    };
    // Tab to the field, then Enter.
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    run(&mut rt, &mut vm, frame_at(None, false), vec![InputEvent::Key { vk: vk::TAB, down: true, repeat: false, mods: Modifiers::NONE }]);
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    run(&mut rt, &mut vm, frame_at(None, false), vec![InputEvent::Key { vk: vk::ENTER, down: true, repeat: false, mods: Modifiers::NONE }]);
    run(&mut rt, &mut vm, frame_at(None, false), vec![]);
    assert_eq!(rt.with_component::<ConfirmBox, _>("confirm", |c| c.accepted), Some(1));
}

thread_local! {
    /// What the [`Probe`]s received: mouse downs, wheel turns, key downs.
    static PROBED: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A custom control that records the routed events its overrides receive.
#[derive(Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
pub struct Probe {
    base: ControlCore,
}

impl Control for Probe {
    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        PROBED.with(|p| p.borrow_mut().push("down"));
        self.base_mut().on_mouse_down(e);
    }
    fn on_mouse_wheel(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        PROBED.with(|p| p.borrow_mut().push("wheel"));
        self.base_mut().on_mouse_wheel(e);
    }
    fn on_key_down(&mut self, e: &mut EventCx<'_, KeyEventArgs>) {
        PROBED.with(|p| p.borrow_mut().push("key"));
        self.base_mut().on_key_down(e);
    }
    fn can_select(&self) -> bool {
        true
    }
}

/// A user control holding a [`Probe`] and a context menu of its own.
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/pane.kbcontrol")]
pub struct Pane {
    base: UserControlCore,
    #[property]
    pub downs: u32,
    #[property]
    pub openings: u32,
    #[property]
    pub copies: u32,
}

thread_local! {
    /// The `Pane` handlers that ran (the user control's own code, not the page's).
    static PANE_RAN: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[kubuno_desktop_views::event_handlers]
impl Pane {
    fn probe_down(&mut self) {
        self.downs += 1;
        PANE_RAN.with(|p| p.borrow_mut().push("probe_down"));
    }
    fn menu_opening(&mut self) {
        self.openings += 1;
        PANE_RAN.with(|p| p.borrow_mut().push("menu_opening"));
    }
    fn copy_click(&mut self) {
        self.copies += 1;
        PANE_RAN.with(|p| p.borrow_mut().push("copy_click"));
    }
}

/// Runs one 300×200 frame of `rt` with `input`.
fn frame_with(rt: &mut kubuno_desktop_views::runtime::Runtime, vm: &mut kubuno_desktop_views::binding::MapViewModel, f: kubuno_desktop_controls::host::Frame, input: Vec<kubuno_desktop_controls::host::InputEvent>) {
    kubuno_desktop_controls::host::input::set_frame_events(input);
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
    rt.frame(&canvas, &f, vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    kubuno_desktop_controls::host::input::set_frame_events(Vec::new());
}

/// Found by the chat migration: a custom control inside a user control (inside a `<Repeater>` item) was never
/// registered with the window's input router, so it got no mouse, wheel or key events. Its events now reach its
/// overrides, and the handlers its element names run on the user control.
#[test]
fn a_custom_control_inside_a_user_control_inside_a_repeater_gets_its_routed_events() {
    use kubuno_desktop_controls::host::{vk, InputEvent, Modifiers};
    PROBED.with(|p| p.borrow_mut().clear());
    PANE_RAN.with(|p| p.borrow_mut().clear());
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(
        rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Repeater ItemsSource="{Binding Items}" ItemHeight="100" X="0" Y="0" Width="300" Height="200"><Pane/></Repeater></Panel>"#),
        "{:?}",
        rt.diagnostics()
    );
    let rows: Vec<kubuno_desktop_views::binding::Row> = vec![kubuno_desktop_views::binding::Row::new().with("Id", Value::F32(0.0))];
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    let at = Some((50.0, 20.0));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(at, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(at, true), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(at, false), vec![]);
    let mut wheel = frame_at(at, false);
    wheel.wheel = (0.0, -120.0);
    frame_with(&mut rt, &mut vm, wheel, vec![]);
    frame_with(&mut rt, &mut vm, frame_at(at, false), vec![InputEvent::Key { vk: vk::DOWN, down: true, repeat: false, mods: Modifiers::NONE }]);
    frame_with(&mut rt, &mut vm, frame_at(at, false), vec![]);
    let probed = PROBED.with(|p| p.borrow().clone());
    for wanted in ["down", "wheel", "key"] {
        assert!(probed.contains(&wanted), "the probe got {wanted}: {probed:?}");
    }
    assert_eq!(PANE_RAN.with(|p| p.borrow().clone()), ["probe_down"], "the handler its element names ran on the user control");
}

thread_local! {
    /// What the page's handlers of [`a_user_control_written_inside_a_repeater_gets_its_click_and_its_own_events`] saw.
    static PAGE_RAN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A user control whose class overrides `on_mouse_up` to raise an event of its own (the DriveCard of the sample).
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/pane.kbcontrol")]
#[kubuno(overrides(Control))]
pub struct TileCard {
    base: UserControlCore,
    /// Occurs on a right click.
    #[event]
    pub menu_requested: Event<EmptyEventArgs>,
}

impl Control for TileCard {
    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.base_mut().on_mouse_up(e);
        if e.args().button == MouseButton::Right {
            self.raise_menu_requested(EmptyEventArgs);
        }
    }
}

#[kubuno_desktop_views::event_handlers]
impl TileCard {
    fn probe_down(&mut self) {}
    fn menu_opening(&mut self) {}
    fn copy_click(&mut self) {}
}

/// Found live (the DriveCard sample): a user control got neither its `Click` nor the events its class raises from its
/// `on_mouse_…` overrides at run time — its own view's root covered it, and the router left `Click` to a node that
/// never raised it. A click on a label of its view is the label's, as in Windows Forms.
#[test]
fn a_user_control_written_inside_a_repeater_gets_its_click_and_its_own_events() {
    let view = r#"<Panel DesignWidth="300" DesignHeight="200"><Repeater ItemsSource="{Binding Items}" ItemHeight="100" X="0" Y="0" Width="300" Height="200"><TileCard OnClick="clicked" OnMenuRequested="menu"/></Repeater></Panel>"#;
    assert_eq!(clicks_on_tile_cards(view), ["clicked1", "menu0"]);
}

/// A user control subscribed to its own `Click` (`<UserControl OnClick="…">`, Windows Forms' `this.Click += …`).
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/self_click.kbcontrol")]
pub struct SelfClick {
    base: UserControlCore,
    #[property]
    pub clicks: u32,
}

#[kubuno_desktop_views::event_handlers]
impl SelfClick {
    fn self_click(&mut self) {
        self.clicks += 1;
    }
}

/// The handler a user control's own view root names runs on the user control, after the page's for the element using
/// it (and the root does not hide the user control from the pointer).
#[test]
fn a_user_control_s_own_click_subscription_runs_with_the_page_s() {
    PAGE_RAN.with(|p| p.borrow_mut().clear());
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><SelfClick x:Name="me" X="0" Y="0" Width="300" Height="100" OnClick="clicked"/></Panel>"#), "{:?}", rt.diagnostics());
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    handlers.insert("clicked", Box::new(|_vm: &mut dyn kubuno_desktop_views::binding::ViewModel, _v: Value| PAGE_RAN.with(|p| p.borrow_mut().push("clicked".into()))));
    let at = Some((250.0, 50.0));
    for f in [frame_at(None, false), frame_at(at, false), frame_at(at, true), frame_at(at, false), frame_at(None, false)] {
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &f, &mut vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    }
    assert_eq!(PAGE_RAN.with(|p| p.borrow().clone()), ["clicked"]);
    assert_eq!(rt.with_component::<SelfClick, _>("me", |c| c.clicks), Some(1));
}

/// The same user control placed on the page (the reference behaviour).
#[test]
fn a_user_control_on_a_page_gets_its_click_and_its_own_events() {
    let view = r#"<Panel DesignWidth="300" DesignHeight="200"><TileCard X="0" Y="100" Width="300" Height="100" OnClick="clicked"/><TileCard X="0" Y="0" Width="300" Height="100" OnClick="clicked" OnMenuRequested="menu"/></Panel>"#;
    let none = usize::MAX;
    assert_eq!(clicks_on_tile_cards(view), [format!("clicked{none}"), format!("menu{none}")]);
}

/// Clicks a page of two [`TileCard`]s (one at the top, one below): its second card's surface, the first card's label,
/// then a right click on the first card; what the page's handlers saw (`<handler><item index>`).
fn clicks_on_tile_cards(view: &str) -> Vec<String> {
    PAGE_RAN.with(|p| p.borrow_mut().clear());
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let rows: Vec<kubuno_desktop_views::binding::Row> = (0..2).map(|i| kubuno_desktop_views::binding::Row::new().with("Id", Value::F32(i as f32))).collect();
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    let mut handlers = kubuno_desktop_views::binding::HandlerTable::new();
    for name in ["clicked", "menu"] {
        handlers.insert(
            name,
            Box::new(move |_vm: &mut dyn kubuno_desktop_views::binding::ViewModel, _v: Value| {
                let item = kubuno_desktop_views::binding::current_item().map(|i| i.index).unwrap_or(usize::MAX);
                PAGE_RAN.with(|p| p.borrow_mut().push(format!("{name}{item}")));
            }),
        );
    }
    let mut run = |rt: &mut kubuno_desktop_views::runtime::Runtime, vm: &mut kubuno_desktop_views::binding::MapViewModel, f| {
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        rt.frame(&canvas, &f, vm, &mut handlers, kubuno_desktop_ui::Rect::new(0.0, 0.0, 300.0, 200.0));
    };
    // A click on the second card's own surface, then one on the label of the first card's view, then a right click.
    for at in [(250.0, 150.0), (50.0, 75.0)] {
        run(&mut rt, &mut vm, frame_at(None, false));
        run(&mut rt, &mut vm, frame_at(Some(at), false));
        run(&mut rt, &mut vm, frame_at(Some(at), true));
        run(&mut rt, &mut vm, frame_at(Some(at), false));
    }
    let mut right = frame_at(Some((250.0, 50.0)), false);
    right.right_down = true;
    run(&mut rt, &mut vm, frame_at(Some((250.0, 50.0)), false));
    run(&mut rt, &mut vm, right);
    run(&mut rt, &mut vm, frame_at(Some((250.0, 50.0)), false));
    run(&mut rt, &mut vm, frame_at(None, false));
    PAGE_RAN.with(|p| p.borrow().clone())
}

/// A `<ContextMenu>` declared in a user control's own view opens on a right click of an element naming it and from
/// code, and its handlers run on the user control.
#[test]
fn a_context_menu_of_a_user_control_opens_by_right_click_and_from_code() {
    use kubuno_desktop_controls::host::{vk, InputEvent, Modifiers};
    PANE_RAN.with(|p| p.borrow_mut().clear());
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Pane x:Name="pane" X="0" Y="0" Width="300" Height="100"/></Panel>"#), "{:?}", rt.diagnostics());
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new();
    let on_label = Some((50.0, 75.0));
    let mut right = frame_at(on_label, false);
    right.right_down = true;
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), vec![]);
    frame_with(&mut rt, &mut vm, right, vec![]);
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), vec![]);
    let key = |k| vec![InputEvent::Key { vk: k, down: true, repeat: false, mods: Modifiers::NONE }];
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), key(vk::DOWN));
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), key(vk::ENTER));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    assert_eq!(rt.with_component::<Pane, _>("pane", |p| (p.openings, p.copies)), Some((1, 1)), "{:?}", PANE_RAN.with(|p| p.borrow().clone()));
    // From code: the user control's own name for its menu.
    kubuno_desktop_views::window::show_context_menu("menu", kubuno_desktop_views::window::MenuAnchor::Point(20.0, 20.0));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(None, false), key(vk::DOWN));
    frame_with(&mut rt, &mut vm, frame_at(None, false), key(vk::ENTER));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    assert_eq!(rt.with_component::<Pane, _>("pane", |p| (p.openings, p.copies)), Some((2, 2)), "{:?}", PANE_RAN.with(|p| p.borrow().clone()));
}

/// The menus of a user control shown through `<Repeater ItemTemplate="…">`: one per item, run on the item's instance.
#[test]
fn a_context_menu_of_an_item_template_user_control_opens_on_its_item() {
    use kubuno_desktop_controls::host::{vk, InputEvent, Modifiers};
    PANE_RAN.with(|p| p.borrow_mut().clear());
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(
        rt.reload_from_text(r#"<Panel DesignWidth="300" DesignHeight="200"><Repeater ItemsSource="{Binding Items}" ItemTemplate="Pane" ItemKey="Id" ItemHeight="100" X="0" Y="0" Width="300" Height="200"/></Panel>"#),
        "{:?}",
        rt.diagnostics()
    );
    let rows: Vec<kubuno_desktop_views::binding::Row> = (0..2).map(|i| kubuno_desktop_views::binding::Row::new().with("Id", Value::F32(i as f32))).collect();
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    let on_label = Some((50.0, 75.0));
    let mut right = frame_at(on_label, false);
    right.right_down = true;
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), vec![]);
    frame_with(&mut rt, &mut vm, right, vec![]);
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), vec![]);
    let key = |k| vec![InputEvent::Key { vk: k, down: true, repeat: false, mods: Modifiers::NONE }];
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), key(vk::DOWN));
    frame_with(&mut rt, &mut vm, frame_at(on_label, false), key(vk::ENTER));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    assert_eq!(PANE_RAN.with(|p| p.borrow().clone()), ["menu_opening", "copy_click"]);
}

/// Found by the chat migration: the elements of a user control's own view reused the page's ids, so the
/// accessibility tree had duplicate node ids and AccessKit refused it (no UI Automation element at all). Every
/// instance's elements now have ids of their own, in a `<Repeater>` too.
#[test]
fn user_controls_and_repeater_items_publish_unique_accessibility_ids() {
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    assert!(
        rt.reload_from_text(
            r#"<Panel DesignWidth="300" DesignHeight="200"><Pane x:Name="a" X="0" Y="0" Width="300" Height="100"/><Repeater ItemsSource="{Binding Items}" ItemHeight="100" X="0" Y="100" Width="300" Height="300"><Pane/></Repeater></Panel>"#
        ),
        "{:?}",
        rt.diagnostics()
    );
    let rows: Vec<kubuno_desktop_views::binding::Row> = (0..3).map(|i| kubuno_desktop_views::binding::Row::new().with("Id", Value::F32(i as f32))).collect();
    let mut vm = kubuno_desktop_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    frame_with(&mut rt, &mut vm, frame_at(None, false), vec![]);
    let tree = kubuno_desktop_controls::host::access::last_published().expect("the accessibility tree is published");
    let mut seen = std::collections::HashSet::new();
    let duplicates: Vec<u64> = tree.nodes.iter().map(|n| n.id).filter(|id| !seen.insert(*id)).collect();
    assert!(duplicates.is_empty(), "duplicate ids {duplicates:?} in {} nodes", tree.nodes.len());
    assert!(tree.nodes.iter().filter(|n| n.name == "Pane").count() >= 2, "the labels of several panes are there: {:?}", tree.nodes.iter().map(|n| &n.name).collect::<Vec<_>>());
    let update = kubuno_desktop_controls::host::access::tree_update(&tree);
    assert_eq!(update.nodes.len(), tree.nodes.len() + 1, "nothing left out");
}
