//! A control clips its children to its own box, as every Windows Forms control does: the elements of a user
//! control's view never paint, nor take the pointer, outside the instance using it — on a page, in a `<Repeater>`
//! item, inside another user control — and a container's children never leave it either.

use kubuno_controls::host::{self, Frame, Modifiers};
use kubuno_ui::graphics::testing::RecordingCanvas;
use kubuno_ui::Rect;
use kubuno_views::prelude::*;

thread_local! {
    /// The `OverflowBox` handlers that ran.
    static RAN: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A user control whose own view (300 × 160) is larger than the instances below: its `spill` button lies partly
/// outside them.
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/overflow_box.kbcontrol")]
pub struct OverflowBox {
    base: UserControlCore,
}

#[kubuno_views::event_handlers]
impl OverflowBox {
    fn inside_click(&mut self) {
        RAN.with(|r| r.borrow_mut().push("inside"));
    }
    fn spill_click(&mut self) {
        RAN.with(|r| r.borrow_mut().push("spill"));
    }
}

/// A user control smaller than the `OverflowBox` it holds.
#[derive(UserControl, Default)]
#[user_control(view = "fixtures/nest_box.kbcontrol")]
pub struct NestBox {
    base: UserControlCore,
}

const WINDOW: Rect = Rect { left: 0.0, top: 0.0, right: 400.0, bottom: 300.0 };

fn frame_at(mouse: Option<(f32, f32)>, down: bool) -> Frame {
    let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
    Frame {
        size: (WINDOW.right, WINDOW.bottom),
        mouse: (x, y),
        mouse_down: down,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, WINDOW.right, WINDOW.bottom),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 1,
        window_focused: true,
    }
}

fn runtime(view: &str) -> kubuno_views::runtime::Runtime {
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    rt
}

/// Paints one frame and returns the canvas calls.
fn paint(rt: &mut kubuno_views::runtime::Runtime, vm: &mut kubuno_views::binding::MapViewModel, f: Frame) -> Vec<String> {
    let mut handlers = kubuno_views::binding::HandlerTable::new();
    let canvas = RecordingCanvas::new();
    rt.frame(&canvas, &f, vm, &mut handlers, WINDOW);
    canvas.calls()
}

/// The rectangle a recorded call names (`fill_rounded(1,2,3,4 r=2)` → 1,2,3,4).
fn rect_of(call: &str) -> Option<Rect> {
    call.split(['(', ')', ' ']).find_map(|token| {
        let v: Vec<f32> = token.split(',').map(|n| n.parse::<f32>()).collect::<Result<_, _>>().ok()?;
        (v.len() == 4).then(|| Rect::new(v[0], v[1], v[2], v[3]))
    })
}

fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::new(a.left.max(b.left), a.top.max(b.top), a.right.min(b.right), a.bottom.min(b.bottom))
}

/// The calls among `calls` whose name contains `needle` (`text("Spill"`), with what they really cover: their
/// rectangle cut by the clips in force when they were made.
fn visible(calls: &[String], needle: &str) -> Vec<(String, Rect)> {
    let mut clips: Vec<Rect> = Vec::new();
    let mut out = Vec::new();
    for call in calls {
        if call.starts_with("push_clip(") {
            clips.push(rect_of(call).unwrap_or(WINDOW));
            continue;
        }
        if call == "pop_clip" {
            clips.pop();
            continue;
        }
        if !call.contains(needle) {
            continue;
        }
        let Some(r) = rect_of(call) else { continue };
        let v = clips.iter().fold(r, |v, c| intersect(v, *c));
        if v.right > v.left && v.bottom > v.top {
            out.push((call.clone(), v));
        }
    }
    out
}

fn inside(v: Rect, area: Rect) -> bool {
    v.left >= area.left - 0.01 && v.top >= area.top - 0.01 && v.right <= area.right + 0.01 && v.bottom <= area.bottom + 0.01
}

/// The calls whose own rectangle lies in `area` (inflated by a focus ring), with what they really cover.
fn drawn_in(calls: &[String], area: Rect) -> Vec<(String, Rect)> {
    let outer = Rect::new(area.left - 4.0, area.top - 4.0, area.right + 4.0, area.bottom + 4.0);
    visible(calls, "").into_iter().filter(|(call, _)| rect_of(call).is_some_and(|r| inside(r, outer))).collect()
}

/// Every call painting the `Spill` button (its text, and the button face around the same box).
fn spill_calls(calls: &[String]) -> Vec<(String, Rect)> {
    let text = visible(calls, "text(\"Spill\"");
    assert!(!text.is_empty(), "the visible part of the spill button is painted: {calls:#?}");
    text
}

/// Clicks at `at` (press, release) and returns the `OverflowBox` handlers that ran.
fn click(rt: &mut kubuno_views::runtime::Runtime, vm: &mut kubuno_views::binding::MapViewModel, at: (f32, f32)) -> Vec<&'static str> {
    RAN.with(|r| r.borrow_mut().clear());
    paint(rt, vm, frame_at(Some(at), false));
    paint(rt, vm, frame_at(Some(at), true));
    paint(rt, vm, frame_at(Some(at), false));
    paint(rt, vm, frame_at(None, false));
    RAN.with(|r| r.borrow().clone())
}

#[test]
fn a_user_control_paints_its_view_inside_its_own_box_only() {
    let mut rt = runtime(r#"<Panel DesignWidth="400" DesignHeight="300"><OverflowBox x:Name="box" X="0" Y="0" Width="200" Height="100"/></Panel>"#);
    let mut vm = kubuno_views::binding::MapViewModel::new();
    let calls = paint(&mut rt, &mut vm, frame_at(None, false));
    let instance = Rect::new(0.0, 0.0, 200.0, 100.0);
    // Every primitive of the spill button (its face, its text…), cut by the clips in force, stays in the instance.
    let spill = Rect::new(150.0, 60.0, 290.0, 140.0);
    let faces = drawn_in(&calls, spill);
    assert!(!faces.is_empty(), "{calls:#?}");
    for (call, v) in faces.into_iter().chain(spill_calls(&calls)) {
        assert!(inside(v, instance), "{call} covers {v:?}, outside {instance:?}");
    }
}

#[test]
fn the_pointer_outside_a_user_control_never_reaches_its_view() {
    let mut rt = runtime(r#"<Panel DesignWidth="400" DesignHeight="300"><OverflowBox x:Name="box" X="0" Y="0" Width="200" Height="100"/></Panel>"#);
    let mut vm = kubuno_views::binding::MapViewModel::new();
    paint(&mut rt, &mut vm, frame_at(None, false));
    // On the part of the spill button outside the instance: nothing.
    assert_eq!(click(&mut rt, &mut vm, (250.0, 120.0)), Vec::<&str>::new());
    // On its visible part: its click.
    assert_eq!(click(&mut rt, &mut vm, (175.0, 80.0)), ["spill"]);
    assert_eq!(click(&mut rt, &mut vm, (30.0, 20.0)), ["inside"]);
}

#[test]
fn a_repeater_item_clips_its_user_control_to_the_item() {
    let mut rt = runtime(
        r#"<Panel DesignWidth="400" DesignHeight="300"><Repeater ItemsSource="{Binding Items}" ItemTemplate="OverflowBox" ItemKey="Id" ItemHeight="100" ItemWidth="200" X="0" Y="0" Width="400" Height="300"/></Panel>"#,
    );
    let rows: Vec<kubuno_views::binding::Row> = (0..2).map(|i| kubuno_views::binding::Row::new().with("Id", Value::F32(i as f32))).collect();
    let mut vm = kubuno_views::binding::MapViewModel::new().with("Items", Value::from(rows));
    let calls = paint(&mut rt, &mut vm, frame_at(None, false));
    let spills = spill_calls(&calls);
    assert_eq!(spills.len(), 2, "{spills:#?}");
    for (i, (call, v)) in spills.iter().enumerate() {
        let item = Rect::new(0.0, 100.0 * i as f32, 200.0, 100.0 * (i + 1) as f32);
        assert!(inside(*v, item), "item {i}: {call} covers {v:?}, outside {item:?}");
    }
    // The first item's spill button, below its item (over the second item's empty area): nothing.
    assert_eq!(click(&mut rt, &mut vm, (175.0, 120.0)), Vec::<&str>::new());
    assert_eq!(click(&mut rt, &mut vm, (175.0, 80.0)), ["spill"]);
}

#[test]
fn a_user_control_inside_a_smaller_one_is_clipped_by_both() {
    let mut rt = runtime(r#"<Panel DesignWidth="400" DesignHeight="300"><NestBox x:Name="nest" X="20" Y="20" Width="170" Height="90"/></Panel>"#);
    let mut vm = kubuno_views::binding::MapViewModel::new();
    let calls = paint(&mut rt, &mut vm, frame_at(None, false));
    let nest = Rect::new(20.0, 20.0, 190.0, 110.0);
    for (call, v) in spill_calls(&calls) {
        assert!(inside(v, nest), "{call} covers {v:?}, outside {nest:?}");
    }
    assert_eq!(click(&mut rt, &mut vm, (185.0, 105.0)), ["spill"]);
    assert_eq!(click(&mut rt, &mut vm, (195.0, 105.0)), Vec::<&str>::new(), "outside the outer user control");
}

#[test]
fn a_panel_clips_its_children() {
    let mut rt = runtime(
        r#"<Panel DesignWidth="400" DesignHeight="300"><Panel x:Name="small" X="0" Y="0" Width="100" Height="50"><Button x:Name="wide" Text="Wide" OnClick="wide_click" X="60" Y="10" Width="100" Height="30"/></Panel></Panel>"#,
    );
    let mut vm = kubuno_views::binding::MapViewModel::new();
    let calls = paint(&mut rt, &mut vm, frame_at(None, false));
    let text = visible(&calls, "text(\"Wide\"");
    assert!(!text.is_empty(), "{calls:#?}");
    let panel = Rect::new(0.0, 0.0, 100.0, 50.0);
    for (call, v) in text {
        assert!(inside(v, panel), "{call} covers {v:?}, outside {panel:?}");
    }
}

#[test]
fn the_designer_selects_only_the_visible_part_of_a_clipped_control() {
    let mut rt = runtime(
        r#"<Panel DesignWidth="400" DesignHeight="300"><Panel x:Name="small" X="0" Y="0" Width="100" Height="50"><Button x:Name="wide" Text="Wide" X="60" Y="10" Width="100" Height="30"/></Panel></Panel>"#,
    );
    let mut vm = kubuno_views::binding::MapViewModel::new();
    let mut handlers = kubuno_views::binding::HandlerTable::new();
    let mut map = kubuno_views::design::LayoutMap::new();
    let canvas = RecordingCanvas::new();
    let _ = rt.frame_with_design(&canvas, &frame_at(None, false), &mut vm, &mut handlers, WINDOW, Some(&mut map));
    let wide = map.entries().iter().find(|e| e.id == "0.0").map(|e| e.id.clone()).expect("the button is recorded");
    assert_eq!(map.hit_test(80.0, 20.0).map(|e| e.id.clone()), Some(wide.clone()), "its visible part selects it");
    assert_ne!(map.hit_test(130.0, 20.0).map(|e| e.id.clone()), Some(wide.clone()), "beside the panel: not the button");
    // Its adorners still frame its whole box.
    assert_eq!(map.get(&wide).map(|e| e.bounds), Some(Rect::new(60.0, 10.0, 160.0, 40.0)));
}

#[test]
fn assistive_technology_gets_the_visible_part_of_a_clipped_control() {
    let mut rt = runtime(r#"<Panel DesignWidth="400" DesignHeight="300"><OverflowBox x:Name="box" X="0" Y="0" Width="200" Height="100"/></Panel>"#);
    let mut vm = kubuno_views::binding::MapViewModel::new();
    paint(&mut rt, &mut vm, frame_at(None, false));
    paint(&mut rt, &mut vm, frame_at(None, false));
    let tree = kubuno_controls::host::access::last_published().expect("the accessibility tree is published");
    let spill = tree.nodes.iter().find(|n| n.name == "Spill").expect("the spill button is announced");
    assert_eq!(spill.bounds, (150.0, 60.0, 200.0, 100.0), "cut to the user control");
}