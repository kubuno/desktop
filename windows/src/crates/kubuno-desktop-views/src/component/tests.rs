//! Tests of the control hierarchy (EVT-7a): upcasts and downcasts, override + event composition
//! order (with and without a `.kbview` sink), delegation to the base, level methods, the property
//! setters, the key and message hooks, and the router delivering events through a control.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop_controls::host::{vk, Frame, InputEvent, Modifiers};
use kubuno_desktop_ui::{FocusId, FocusRing, Rect};

use super::*;
use crate::binding::{HandlerTable, MapViewModel, Value};
use crate::controls::{Button, CheckBox, Label, ListBox, Slider, TextField};
use crate::events::router::{Dispatch, FrameInput, InputRouter, SlotEvents};
use crate::events::{
    ChangeSource, CheckedChangedEventArgs, ElementRef, EmptyEventArgs, EventArgs, Key, KeyEventArgs, MouseEventArgs, TextChangedEventArgs,
};
use crate::node::{InteractCx, ViewEvent, ViewEventKind};

type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn push(log: &Log, s: impl Into<String>) {
    log.borrow_mut().push(s.into());
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

/// The design note's proof shape: extends `Button`, overrides `on_click` (around its base) and a
/// few hooks.
#[derive(Component)]
#[kubuno(extends = Button, overrides(Control))]
struct RoundButton {
    base: Button,
    log: Log,
    /// `on_click` does not call its base (the event is suppressed).
    suppress: bool,
    /// Ctrl+S is a command key.
    eat_ctrl_s: bool,
    /// `wnd_proc` eats left-button presses.
    eat_presses: bool,
}

impl RoundButton {
    fn new(log: &Log) -> Self {
        Self { base: Button::new("Round"), log: log.clone(), suppress: false, eat_ctrl_s: false, eat_presses: false }
    }
}

impl Control for RoundButton {
    fn on_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        push(&self.log, "override:before");
        if !self.suppress {
            self.base_mut().on_click(e);
        }
        push(&self.log, "override:after");
    }

    fn on_paint(&mut self, _e: &mut PaintEventCx<'_>) {
        push(&self.log, "paint");
    }

    fn process_cmd_key(&mut self, _msg: &mut Message, key: Keys) -> bool {
        let hit = self.eat_ctrl_s && key == Keys::ctrl(Key::letter('s'));
        if hit {
            push(&self.log, "cmd:Ctrl+S");
        }
        hit
    }

    fn wnd_proc(&mut self, msg: &mut Message) -> bool {
        if self.eat_presses && msg.msg == Message::WM_LBUTTONDOWN {
            push(&self.log, format!("wndproc:{:?}", msg.point()));
            return true;
        }
        false
    }
}

/// A class of your own as a base (`levels(…)`), with no override: everything delegates.
#[derive(Component)]
#[kubuno(extends = RoundButton, levels(ButtonBase))]
struct BigRoundButton {
    base: RoundButton,
}

/// A control written from the `Control` level up.
#[derive(Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
struct Gauge {
    #[kubuno(base)]
    core: ControlCore,
    seen: Vec<&'static str>,
}

impl Control for Gauge {
    fn on_event(&mut self, event: &'static str, e: &mut EventCx<'_, dyn EventArgs>) {
        self.seen.push(event);
        self.base_mut().on_event(event, e);
    }

    fn is_input_key(&self, key: Keys) -> bool {
        key.key == Key(vk::DOWN)
    }

    fn process_dialog_key(&mut self, key: Keys) -> bool {
        self.seen.push("dialog");
        key.key == Key(vk::ESCAPE)
    }
}

/// A checkbox that overrides a level method (`ButtonBase::on_checked_changed`).
#[derive(Component, Default)]
#[kubuno(extends = CheckBox, overrides(ButtonBase))]
struct TriCheck {
    base: CheckBox,
    changes: Vec<bool>,
}

impl ButtonBase for TriCheck {
    fn on_checked_changed(&mut self, e: &mut EventCx<'_, CheckedChangedEventArgs>) {
        self.changes.push(e.new);
        self.base_mut().on_checked_changed(e);
    }
}

/// A non-visual component.
#[derive(Component, Default)]
#[kubuno(extends = Component)]
struct Clock {
    base: ComponentCore,
}

/// A sink standing for a `.kbview` element's handler.
struct TestSink {
    log: Log,
    handled: bool,
}

impl RaiseSink for TestSink {
    fn sender(&self) -> ElementRef<'_> {
        ElementRef::detached("xml")
    }

    fn raise(&mut self, event: &'static str, args: &mut dyn EventArgs) {
        push(&self.log, format!("sink:{event}"));
        if self.handled {
            if let Some(h) = args.as_handled_mut() {
                h.set_handled(true);
            }
        }
    }
}

// ── Types ───────────────────────────────────────────────────────────────────────────────

#[test]
fn upcasts_downcasts_and_the_chain() {
    let l = log();
    let rb = RoundButton::new(&l);
    let bb: &dyn ButtonBase = &rb;
    let control: &dyn Control = bb;
    let component: &dyn Component = control;
    assert!(component.is::<RoundButton>() && !component.is::<Button>());
    assert_eq!(component.class_name(), "RoundButton");
    assert_eq!(component.class_chain(), ["RoundButton", "Button", "ButtonBase", "Control", "Component"]);
    assert!(component.is_a("Button") && component.is_a("ButtonBase") && !component.is_a("ListControl"));
    assert!(control.downcast_ref::<RoundButton>().is_some() && control.downcast_ref::<Label>().is_none());
    assert!(bb.is_a("Control"));
    assert_eq!(component.find_base::<Button>().map(|b| b.text().to_string()), Some("Round".to_string()));
    assert!(component.find_base::<Label>().is_none());
    assert!(component.as_button_base().is_some() && component.as_list_control().is_none());
    assert!(component.base_component().is_some_and(|b| b.is::<Button>()));
    assert!(Button::new("x").as_component().base_component().is_some_and(|b| b.class_name() == "ButtonBase"), "a built-in class extends a level's core");
    assert_eq!(<BigRoundButton as Lineage>::CHAIN, ["BigRoundButton", "RoundButton", "Button", "ButtonBase", "Control", "Component"]);
    assert_eq!(<BigRoundButton as ClassInfo>::NAME, "BigRoundButton");

    let g = Gauge::default();
    assert_eq!(g.class_chain(), ["Gauge", "Control", "Component"]);
    assert!(g.base_control().is_some_and(|b| b.class_name() == "Control") && g.as_button_base().is_none());
    let c = Clock::default();
    assert!(c.as_control().is_none() && c.class_chain() == ["Clock", "Component"]);
}

#[test]
fn a_shared_core_serves_every_level() {
    let l = log();
    let mut rb = RoundButton::new(&l);
    rb.set_text("Pill");
    assert_eq!(rb.base().text(), "Pill", "the base object's core is the derived object's core");
    rb.button_base_core_mut().is_default = true;
    assert!(rb.is_default() && rb.base().is_default());
    assert_eq!(rb.control_core().text, "Pill");
}

// ── Override + event composition ────────────────────────────────────────────────────────

#[test]
fn an_override_runs_around_its_base_raise() {
    let l = log();
    let mut rb = RoundButton::new(&l);
    let sub = l.clone();
    rb.click().subscribe(move |_, _| push(&sub, "subscriber")).detach();
    rb.perform_click();
    assert_eq!(take(&l), ["override:before", "subscriber", "override:after"]);
}

#[test]
fn the_sink_runs_before_the_rust_subscribers() {
    let l = log();
    let mut rb = RoundButton::new(&l);
    let sub = l.clone();
    rb.click().subscribe(move |sender, _| push(&sub, format!("subscriber:{:?}", sender.name))).detach();
    let mut sink = TestSink { log: l.clone(), handled: false };
    let mut args = MouseEventArgs::default();
    rb.dispatch_event("OnClick", &mut EventCx::with_sink(&mut args, &mut sink));
    assert_eq!(take(&l), ["override:before", "sink:OnClick", "subscriber:Some(\"xml\")", "override:after"]);
}

#[test]
fn an_override_that_skips_its_base_suppresses_the_event() {
    let l = log();
    let mut rb = RoundButton::new(&l);
    rb.suppress = true;
    let sub = l.clone();
    rb.click().subscribe(move |_, _| push(&sub, "subscriber")).detach();
    let mut sink = TestSink { log: l.clone(), handled: false };
    let mut args = MouseEventArgs::default();
    rb.dispatch_event("OnClick", &mut EventCx::with_sink(&mut args, &mut sink));
    assert_eq!(take(&l), ["override:before", "override:after"]);
}

#[test]
fn handled_in_the_sink_stops_the_rust_subscribers() {
    let l = log();
    let mut rb = RoundButton::new(&l);
    let sub = l.clone();
    rb.key_down().subscribe(move |_, _| push(&sub, "subscriber")).detach();
    let mut sink = TestSink { log: l.clone(), handled: true };
    let mut args = KeyEventArgs::new(Key(vk::F5), Modifiers::NONE);
    rb.dispatch_event("OnKeyDown", &mut EventCx::with_sink(&mut args, &mut sink));
    assert_eq!(take(&l), ["sink:OnKeyDown"]);
    assert!(args.handled);
}

#[test]
fn methods_a_class_does_not_override_run_its_base() {
    let l = log();
    let mut big = BigRoundButton { base: RoundButton::new(&l) };
    let sub = l.clone();
    big.mouse_down().subscribe(move |_, e| push(&sub, format!("down:{}", e.clicks))).detach();
    let sub = l.clone();
    big.click().subscribe(move |_, _| push(&sub, "click")).detach();
    big.dispatch_event("OnMouseDown", &mut EventCx::new(&mut MouseEventArgs { clicks: 2, ..Default::default() }));
    // BigRoundButton has no override: its on_click delegates to RoundButton's.
    big.perform_click();
    assert_eq!(take(&l), ["down:2", "override:before", "click", "override:after"]);
}

#[test]
fn level_methods_are_dispatched_virtually() {
    let mut t = TriCheck::default();
    let seen = log();
    let sub = seen.clone();
    t.checked_changed().subscribe(move |_, e| push(&sub, format!("checked:{}", e.new))).detach();
    let mut args = CheckedChangedEventArgs::new(false, true, ChangeSource::User);
    t.dispatch_event("OnCheckedChanged", &mut EventCx::new(&mut args));
    assert_eq!(t.changes, [true]);
    assert_eq!(take(&seen), ["checked:true"]);
    // The same event with other args (a RadioButton's is a TextChanged) takes the generic path.
    let l = log();
    let mut sink = TestSink { log: l.clone(), handled: false };
    let mut other = TextChangedEventArgs::new(String::new(), "a".into(), ChangeSource::User);
    t.dispatch_event("OnCheckedChanged", &mut EventCx::with_sink(&mut other, &mut sink));
    assert_eq!(t.changes, [true], "not the typed level method");
    assert_eq!(take(&l), ["sink:OnCheckedChanged"]);
}

#[test]
fn events_without_a_method_go_through_on_event() {
    let mut g = Gauge::default();
    let l = log();
    let mut sink = TestSink { log: l.clone(), handled: false };
    let mut args = EmptyEventArgs;
    g.dispatch_event("OnStepSelected", &mut EventCx::with_sink(&mut args, &mut sink));
    // A Click whose args are not mouse args (a toolbar item's) is not `on_click`'s either.
    let mut item = crate::events::ItemEventArgs { index: 2 };
    g.dispatch_event("OnClick", &mut EventCx::with_sink(&mut item, &mut sink));
    assert_eq!(g.seen, ["OnStepSelected", "OnClick"]);
    assert_eq!(take(&l), ["sink:OnStepSelected", "sink:OnClick"]);
}

// ── Properties and operations ───────────────────────────────────────────────────────────

#[test]
fn property_setters_raise_their_change_events() {
    let l = log();
    let mut b = Button::new("Old");
    let sub = l.clone();
    b.text_changed().subscribe(move |_, e| push(&sub, format!("text:{}->{} {:?}", e.old, e.new, e.source))).detach();
    let sub = l.clone();
    b.visible_changed().subscribe(move |_, _| push(&sub, "visible")).detach();
    let sub = l.clone();
    b.enabled_changed().subscribe(move |_, _| push(&sub, "enabled")).detach();
    b.set_text("New");
    b.set_text("New");
    b.set_visible(false);
    b.set_visible(false);
    b.set_enabled(false);
    assert_eq!(take(&l), ["text:Old->New Code", "visible", "enabled"]);
    assert!(!b.can_focus() && !b.focus(), "a hidden, disabled control takes no focus");
    let mut clicks = 0;
    let sub = l.clone();
    b.click().subscribe(move |_, _| push(&sub, "click")).detach();
    b.perform_click();
    clicks += take(&l).len();
    assert_eq!(clicks, 0, "PerformClick does nothing on a control that cannot be selected");

    let mut s = Slider::new();
    let sub = l.clone();
    s.value_changed().subscribe(move |_, e| push(&sub, format!("value:{}", e.new))).detach();
    s.set_value(250.0);
    s.set_value(100.0);
    assert_eq!(take(&l), ["value:100"], "clamped to the maximum, then unchanged");

    let mut list = ListBox::new();
    let sub = l.clone();
    list.selection_changed().subscribe(move |_, e| push(&sub, format!("sel:{:?}->{:?}", e.old, e.new))).detach();
    list.set_selected_index(Some(3));
    assert_eq!(take(&l), ["sel:None->Some(3)"]);

    let mut t = TextField::new();
    let sub = l.clone();
    t.read_only_changed().subscribe(move |_, _| push(&sub, "ro")).detach();
    t.set_read_only(true);
    assert!(t.read_only());
    assert_eq!(take(&l), ["ro"]);
}

#[test]
fn dispose_raises_disposed_once() {
    let l = log();
    let mut c = Clock::default();
    let sub = l.clone();
    c.disposed().subscribe(move |_, _| push(&sub, "disposed")).detach();
    c.dispose();
    c.dispose();
    assert!(c.is_disposed());
    assert_eq!(take(&l), ["disposed"]);
}

#[test]
fn styles_bounds_invalidation_and_creation() {
    let mut g = Gauge::default();
    assert!(g.get_style(ControlStyles::SELECTABLE) && g.get_style(ControlStyles::STANDARD_DOUBLE_CLICK));
    g.set_style(ControlStyles::RESIZE_REDRAW | ControlStyles::OPAQUE, true);
    assert!(g.get_style(ControlStyles::OPAQUE));
    g.set_bounds(Rect::new(10.0, 20.0, 110.0, 60.0));
    g.set_bounds_core(Rect::new(0.0, 0.0, 50.0, 5.0), BoundsSpecified::WIDTH);
    let b = g.bounds();
    assert_eq!((b.left, b.top, b.right, b.bottom), (10.0, 20.0, 60.0, 60.0), "only the width changed");
    assert!(g.invalidated_rect().is_none());
    g.on_resize(&mut EventCx::new(&mut EmptyEventArgs));
    assert!(g.invalidated_rect().is_some(), "RESIZE_REDRAW invalidates on resize");
    g.invalidate_rect(Rect::new(0.0, 0.0, 5.0, 5.0));
    assert_eq!(g.invalidated_rect().map(|r| (r.left, r.top)), Some((0.0, 0.0)), "invalid areas accumulate");

    let l = log();
    let sub = l.clone();
    g.handle_created().subscribe(move |_, _| push(&sub, "handle")).detach();
    g.create_control();
    g.create_control();
    assert!(g.created() && g.is_handle_created());
    assert_eq!(take(&l), ["handle"]);

    let mut layouts = 0;
    let sub = l.clone();
    g.layout().subscribe(move |_, _| push(&sub, "layout")).detach();
    g.suspend_layout();
    g.perform_layout();
    g.resume_layout(true);
    layouts += take(&l).len();
    assert_eq!(layouts, 1, "suspended layout runs once, on resume");
    assert!(g.focus(), "a visible, enabled control accepts a focus request");
    assert_eq!(g.create_params().class_name, "Control", "the base window class, as in WinForms");
}

#[test]
fn keys_and_messages_round_trip() {
    let k = Keys::new(Key::letter('s'), Modifiers::CTRL);
    assert_eq!(Message::key(Message::WM_KEYDOWN, k).keys(), Some(k));
    assert_eq!(Message::char('é').char_code(), Some('é'));
    assert_eq!(Message::mouse(Message::WM_MOUSEMOVE, -3.0, 12.4, 0.0).point(), (-3.0, 12.0));
    assert!(Keys::plain(Key(vk::TAB)).is_dialog_key() && !k.is_dialog_key());
    let wheel = Message::mouse(Message::WM_MOUSEWHEEL, 0.0, 0.0, -1.0);
    assert_eq!((wheel.wparam >> 16) as u16 as i16, 120, "a notch up is +WHEEL_DELTA");
}

#[test]
fn an_event_keeps_its_args_type() {
    let map = EventMap::default();
    let _a = map.event::<MouseEventArgs>("OnClick").subscribe(|_, e| e.clicks = 9);
    // Asking with another type gives a detached event (and logs an error).
    let other = map.event::<KeyEventArgs>("OnClick");
    assert_eq!(other.subscriber_count(), 0);
    let mut e = MouseEventArgs::default();
    map.raise("OnClick", &ElementRef::detached("x"), &mut e);
    assert_eq!(e.clicks, 9);
    // Raised with the wrong args: the subscribers are skipped.
    let mut k = KeyEventArgs::new(Key(vk::F1), Modifiers::NONE);
    map.raise("OnClick", &ElementRef::detached("x"), &mut k);
}

#[test]
fn text_classes_take_their_navigation_keys() {
    let field = TextField::new();
    assert!(field.is_input_key(Keys::plain(Key(vk::LEFT))) && !field.is_input_key(Keys::plain(Key(vk::TAB))));
    assert!(!Button::new("b").is_input_key(Keys::plain(Key(vk::LEFT))));
}

// ── The router delivers through the control ─────────────────────────────────────────────

fn frame(mouse: (f32, f32), down: bool) -> Frame {
    Frame {
        size: (400.0, 300.0),
        mouse,
        mouse_down: down,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 400.0, 300.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: if down { 1 } else { 0 },
        window_focused: true,
    }
}

/// One element (`rb`, a RoundButton with every `On*` handler named `rb:<Event>`) painted at a
/// fixed rectangle, driven frame by frame through the real router.
struct Harness {
    router: InputRouter,
    focus: FocusRing,
    vm: MapViewModel,
    handlers: HandlerTable,
    slot: Rc<SlotEvents>,
    control: Rc<RefCell<dyn Component>>,
    log: Log,
}

impl Harness {
    fn new(standard_click: bool) -> (Self, Rc<RefCell<RoundButton>>) {
        let log = log();
        let mut slot = SlotEvents::new("0", "RoundButton");
        slot.name = Some("rb".into());
        slot.focus_id = Some(FocusId::of("rb"));
        slot.native_click = !standard_click;
        slot.keyboard_click = true;
        slot.standard_double_click = false;
        let mut handlers = HandlerTable::new();
        for e in ["OnClick", "OnMouseDown", "OnMouseUp", "OnMouseClick", "OnKeyDown", "OnKeyPress", "OnKeyUp", "OnMouseEnter", "OnGotFocus"] {
            slot = slot.with_handler(e, format!("rb:{}", &e[2..]));
            let l = log.clone();
            let name = format!("rb:{}", &e[2..]);
            handlers.insert(name.clone(), Box::new(move |_vm, _v: Value| push(&l, format!("xml {name}"))));
        }
        let rb = Rc::new(RefCell::new(RoundButton::new(&log)));
        let control: Rc<RefCell<dyn Component>> = rb.clone();
        let mut router = InputRouter::new();
        router.set_root(Rc::new(SlotEvents::new("", "Panel")));
        (Self { router, focus: FocusRing::new(), vm: MapViewModel::new(), handlers, slot: Rc::new(slot), control, log }, rb)
    }

    fn run(&mut self, f: Frame, keys: &[InputEvent]) -> (Vec<ViewEvent>, Vec<usize>) {
        let mut events = Vec::new();
        self.focus.begin_frame(&f);
        let consumed = {
            let mut d = Dispatch { vm: &mut self.vm, handlers: &mut self.handlers, events: &mut events };
            let input = FrameInput { frame: &f, now_ms: 0, events: keys };
            let outcome = self.router.begin_frame(&input, &mut self.focus, &mut d);
            let bounds = Rect::new(10.0, 10.0, 110.0, 40.0);
            self.router.register_with_control(self.slot.clone(), bounds, Some(&self.control));
            if let Some(id) = self.slot.focus_id {
                self.focus.register(id, bounds);
            }
            self.router.end_frame(&mut d);
            outcome.consumed
        };
        self.focus.end_frame();
        (events, consumed)
    }
}

#[test]
fn routed_events_reach_the_override_then_the_handler_then_the_subscribers() {
    let (mut h, rb) = Harness::new(true);
    let sub = h.log.clone();
    rb.borrow().click().subscribe(move |sender, _| push(&sub, format!("rust Click from {:?}", sender.name))).detach();
    let sub = h.log.clone();
    rb.borrow().mouse_down().subscribe(move |_, _| push(&sub, "rust MouseDown")).detach();
    h.run(frame((50.0, 20.0), false), &[]);
    h.run(frame((50.0, 20.0), false), &[]);
    take(&h.log);
    h.run(frame((50.0, 20.0), true), &[]);
    let (events, _) = h.run(frame((50.0, 20.0), false), &[]);
    assert_eq!(
        take(&h.log),
        [
            "xml rb:GotFocus",
            "xml rb:MouseDown",
            "rust MouseDown",
            "override:before",
            "xml rb:Click",
            "rust Click from Some(\"rb\")",
            "override:after",
            "xml rb:MouseClick",
            "xml rb:MouseUp",
        ]
    );
    let names: Vec<&str> = events.iter().filter_map(|e| if let ViewEventKind::Other { name, .. } = e.kind { Some(name) } else { None }).collect();
    assert_eq!(names, ["Click", "MouseClick", "MouseUp"], "reported after the override ran");
}

#[test]
fn a_suppressed_click_is_neither_handled_nor_reported() {
    let (mut h, rb) = Harness::new(true);
    rb.borrow_mut().suppress = true;
    h.run(frame((50.0, 20.0), false), &[]);
    h.run(frame((50.0, 20.0), false), &[]);
    h.run(frame((50.0, 20.0), true), &[]);
    take(&h.log);
    let (events, _) = h.run(frame((50.0, 20.0), false), &[]);
    assert_eq!(take(&h.log), ["override:before", "override:after", "xml rb:MouseClick", "xml rb:MouseUp"]);
    assert!(events.iter().all(|e| !matches!(e.kind, ViewEventKind::Other { name: "Click", .. })));
}

#[test]
fn process_cmd_key_consumes_the_key_before_key_down() {
    let (mut h, rb) = Harness::new(true);
    rb.borrow_mut().eat_ctrl_s = true;
    h.focus.focus(FocusId::of("rb"));
    h.run(frame((300.0, 200.0), false), &[]);
    h.run(frame((300.0, 200.0), false), &[]);
    take(&h.log);
    let keys = [
        InputEvent::Key { vk: vk::letter('s'), down: true, repeat: false, mods: Modifiers::CTRL },
        InputEvent::Text("s".into()),
        InputEvent::Key { vk: vk::F5, down: true, repeat: false, mods: Modifiers::NONE },
    ];
    let (_, consumed) = h.run(frame((300.0, 200.0), false), &keys);
    assert_eq!(take(&h.log), ["cmd:Ctrl+S", "xml rb:KeyDown"]);
    assert_eq!(consumed, [0, 1], "the command key and its character are consumed");
}

#[test]
fn wnd_proc_eats_a_press_before_it_becomes_an_event() {
    let (mut h, rb) = Harness::new(true);
    rb.borrow_mut().eat_presses = true;
    h.run(frame((50.0, 20.0), false), &[]);
    h.run(frame((50.0, 20.0), false), &[]);
    take(&h.log);
    h.run(frame((50.0, 20.0), true), &[]);
    h.run(frame((50.0, 20.0), false), &[]);
    assert_eq!(take(&h.log), ["xml rb:GotFocus", "wndproc:(40.0, 10.0)"], "no MouseDown, no capture, no Click");
}

#[test]
fn dialog_keys_the_control_does_not_take_go_to_process_dialog_key() {
    let mut g = Gauge::default();
    assert!(!g.process_dialog_key(Keys::plain(Key(vk::TAB))));
    assert!(g.process_dialog_key(Keys::plain(Key(vk::ESCAPE))));
    assert!(g.is_input_key(Keys::plain(Key(vk::DOWN))));
}

#[test]
fn a_node_fires_its_own_click_through_the_control() {
    use crate::binding::PropSource;
    let l = log();
    let mut rb = RoundButton::new(&l);
    let sub = l.clone();
    rb.click().subscribe(move |_, _| push(&sub, "rust Click")).detach();
    let mut node = crate::node::ButtonNode::new(
        PropSource::Literal("Ok".into()),
        PropSource::Literal(String::new()),
        PropSource::Literal(String::new()),
        PropSource::Literal(String::new()),
        PropSource::Literal(false),
        None,
        Some("ok_click".into()),
    );
    let mut handlers = HandlerTable::new();
    let hl = l.clone();
    handlers.insert("ok_click", Box::new(move |_vm, v: Value| push(&hl, format!("xml ok_click {v:?}"))));
    let mut vm = MapViewModel::new();
    let mut focus = FocusRing::new();
    let mut events = Vec::new();
    let bounds = Rect::new(0.0, 0.0, 100.0, 30.0);
    for down in [true, false] {
        let f = frame((10.0, 10.0), down);
        let mut ix = InteractCx { frame: &f, vm: &mut vm, focus: &mut focus, handlers: &mut handlers, events: &mut events, sender: None, control: Some(&mut rb), activate: false };
        node.interact(&mut ix, bounds);
    }
    assert_eq!(take(&l), ["override:before", "xml ok_click Bool(true)", "rust Click", "override:after"]);
    assert_eq!(events.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(), [ViewEventKind::Clicked]);
}
