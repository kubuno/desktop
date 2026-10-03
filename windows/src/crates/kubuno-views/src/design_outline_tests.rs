//! What a design surface draws on the elements themselves: nothing but what they draw at run time,
//! plus a faint dashed outline around a container that would otherwise be invisible (Windows Forms'
//! dotted border around a borderless `Panel`). Regression test for the light-blue boxes the paint
//! debug overlay drew around every Label and TextField of a designed view when Visual Studio's
//! Debug > Kubuno > Paint debug was left on (its `KUBUNO_PAINT_DEBUG` reached the design surface).

use super::{container_outline_dashes, set_container_outlines, LayoutMap};
use crate::binding::{HandlerTable, MapViewModel};
use crate::runtime::Runtime;
use kubuno_controls::host::{self, paint_debug, Frame, Modifiers};
use kubuno_ui::graphics::testing::RecordingCanvas;
use kubuno_ui::Rect;

/// A Label, a TextField, a borderless Panel, a borderless Stack, and a Panel with a surface (visible
/// on its own), on the view's root panel.
const VIEW: &str = r#"<Panel DesignWidth="600" DesignHeight="400">
  <Label x:Name="name_label" Text="Name" X="20" Y="20" Width="100" Height="26"/>
  <TextField x:Name="name_field" X="130" Y="16" Width="200" Height="34"/>
  <Panel x:Name="drop_zone" X="20" Y="80" Width="300" Height="200"/>
  <Panel x:Name="card" Surface="Card" X="340" Y="80" Width="200" Height="200"/>
  <Stack x:Name="column" X="20" Y="300" Width="300" Height="80"><Button Text="Go"/></Stack>
</Panel>"#;

/// One frame of `VIEW`, painted in design mode (a layout map recorded) or at run time.
fn render(design: bool) -> (RecordingCanvas, LayoutMap) {
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    let frame = Frame {
        size: (600.0, 400.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 600.0, 400.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let canvas = RecordingCanvas::new();
    let mut map = LayoutMap::new();
    let mut vm = MapViewModel::new();
    let mut handlers = HandlerTable::new();
    rt.frame_with_design(&canvas, &frame, &mut vm, &mut handlers, Rect::new(0.0, 0.0, 600.0, 400.0), design.then_some(&mut map));
    (canvas, map)
}

/// How a `RecordingCanvas` logs a square-cornered fill of `r`.
fn fill(r: &Rect) -> String {
    format!("fill_rounded({},{},{},{} r=0)", r.left, r.top, r.right, r.bottom)
}

/// How many of the dashes a container outline around `bounds` would paint were painted.
fn outline_dashes_painted(canvas: &RecordingCanvas, bounds: Rect) -> usize {
    let calls = canvas.calls();
    container_outline_dashes(bounds).iter().filter(|d| calls.contains(&fill(d))).count()
}

fn bounds_of(map: &LayoutMap, id: &str) -> Rect {
    map.get(id).unwrap_or_else(|| panic!("no layout entry for {id:?}")).bounds
}

#[test]
fn design_mode_draws_controls_as_at_run_time_and_outlines_only_invisible_containers() {
    // The overlay left on, as Visual Studio's Paint debug toggle leaves it in the environment.
    paint_debug::set_flags(paint_debug::PaintDebugFlags::ALL);
    set_container_outlines(true);
    let (design, map) = render(true);
    assert_eq!(paint_debug::layout_note_count(), 0, "a design surface reports no layout bounds to the paint debug overlay");

    let (label, field) = (bounds_of(&map, "0"), bounds_of(&map, "1"));
    let (drop_zone, card, column) = (bounds_of(&map, "2"), bounds_of(&map, "3"), bounds_of(&map, "4"));
    assert_eq!((label.left, label.top, label.right, label.bottom), (20.0, 20.0, 120.0, 46.0));

    // Nothing around the Label, the TextField, the Panel with a surface or the view itself.
    for (name, bounds) in [("Label", label), ("TextField", field), ("Panel with a surface", card), ("root", bounds_of(&map, ""))] {
        assert_eq!(outline_dashes_painted(&design, bounds), 0, "no design outline around the {name}");
        let stroked = format!("{},{},{},{}", bounds.left, bounds.top, bounds.right, bounds.bottom);
        let at_run_time = render(false).0.calls().iter().filter(|c| c.contains(&stroked)).count();
        assert_eq!(design.calls().iter().filter(|c| c.contains(&stroked)).count(), at_run_time, "the {name} paints the same primitives on its box as at run time");
    }
    // The dashed outline, all of it, around the borderless Panel and the borderless Stack.
    for (name, bounds) in [("borderless Panel", drop_zone), ("borderless Stack", column)] {
        let expected = container_outline_dashes(bounds).len();
        assert!(expected > 20, "{expected}");
        assert_eq!(outline_dashes_painted(&design, bounds), expected, "the {name} is outlined");
    }

    // Under the container's children, as on a Windows Forms panel's own surface: the Button that
    // touches the Stack's edges is painted after the dashes, so it covers them.
    let calls = design.calls();
    let button = bounds_of(&map, "4.0");
    let button_box = format!("{},{},{},{}", button.left, button.top, button.right, button.bottom);
    let first_button_call = calls.iter().position(|c| c.contains(&button_box)).expect("the Button paints its box");
    let last_dash = container_outline_dashes(column).iter().filter_map(|d| calls.iter().position(|c| *c == fill(d))).max().expect("dashes");
    assert!(last_dash < first_button_call, "the outline is painted before the Stack's children");

    // At run time: no outline at all.
    let (run, _) = render(false);
    assert_eq!(outline_dashes_painted(&run, drop_zone), 0);
    assert!(paint_debug::layout_note_count() > 0, "the running view still feeds the overlay");

    // The designer option off ("Show design outlines"): none either.
    set_container_outlines(false);
    let (plain, _) = render(true);
    assert_eq!(outline_dashes_painted(&plain, drop_zone), 0);
    assert_eq!(outline_dashes_painted(&plain, column), 0);
    set_container_outlines(true);
    paint_debug::set_flags(paint_debug::PaintDebugFlags::OFF);
}

#[test]
fn the_outline_is_one_dip_thin_dashed_and_inside_the_box() {
    let bounds = Rect::new(10.0, 10.0, 70.0, 40.0);
    let dashes = container_outline_dashes(bounds);
    assert!(!dashes.is_empty());
    for d in &dashes {
        let (w, h) = (d.right - d.left, d.bottom - d.top);
        assert!(w.min(h) == 1.0 && w.max(h) <= 3.0, "1 DIP thick, at most 3 DIP long: {w}x{h}");
        assert!(d.left >= bounds.left && d.top >= bounds.top && d.right <= bounds.right && d.bottom <= bounds.bottom, "inside the box");
    }
    // Dashed, not solid: the top edge is about half ink.
    let top: f32 = dashes.iter().filter(|d| d.top == bounds.top && d.bottom == bounds.top + 1.0).map(|d| d.right - d.left).sum();
    assert!(top > 25.0 && top < 40.0, "{top}");
}
