//! Frame time of long bound lists (10 000 rows): what a frame costs once the list is loaded and
//! nothing changes. Ignored by default (a measurement, not a check); run it with
//! `cargo test -p kubuno-views --release --test perf_lists -- --ignored --nocapture`.
//!
//! The canvas is a `RecordingCanvas` (no GPU), so the numbers are the binding, layout and paint
//! logic of the runtime alone — what the `ItemsSource` change detection is about.

use std::time::Instant;

use kubuno_controls::host::{self, Frame, Modifiers};
use kubuno_ui::graphics::testing::RecordingCanvas;
use kubuno_ui::Rect;
use kubuno_views::binding::{HandlerTable, MapViewModel, Row, Value, ViewModel};
use kubuno_views::runtime::Runtime;

const ROWS: usize = 10_000;

fn rows() -> Vec<Row> {
    (0..ROWS)
        .map(|i| Row::new().with("Text", Value::Str(format!("Item {i}"))).with("Size", Value::Str(format!("{} Ko", i * 3))))
        .collect()
}

fn frame() -> Frame {
    Frame {
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
    }
}

/// The mean time of one frame of `view` (after a warm-up), in microseconds.
fn measure(view: &str, vm: &mut dyn ViewModel) -> f64 {
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let mut handlers = HandlerTable::new();
    let bounds = Rect::new(0.0, 0.0, 600.0, 400.0);
    let f = frame();
    for _ in 0..5 {
        let canvas = RecordingCanvas::new();
        rt.frame(&canvas, &f, vm, &mut handlers, bounds);
    }
    const N: u32 = 60;
    let start = Instant::now();
    for _ in 0..N {
        let canvas = RecordingCanvas::new();
        rt.frame(&canvas, &f, vm, &mut handlers, bounds);
    }
    start.elapsed().as_secs_f64() * 1e6 / f64::from(N)
}

/// A view model that converts its `Vec<Row>` anew at every read (the worst case: a new snapshot
/// of equal rows every frame).
struct CopyingVm(Vec<Row>);

impl ViewModel for CopyingVm {
    fn get(&self, path: &str) -> Option<Value> {
        (path == "Items").then(|| Value::from(self.0.clone()))
    }
    fn set(&mut self, _: &str, _: Value) {}
}

fn report(name: &str, view: &str) {
    let mut vm = MapViewModel::new().with("Items", Value::from(rows()));
    let shared = measure(view, &mut vm);
    let mut copying = CopyingVm(rows());
    let copied = measure(view, &mut copying);
    println!("PERF {name}: {ROWS} rows, {shared:.0} µs per frame (shared rows), {copied:.0} µs (a copy per read)");
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn long_bound_lists_frame_time() {
    report(
        "ListView",
        r#"<Panel DesignWidth="600" DesignHeight="400"><ListView ItemsSource="{Binding Items}" X="0" Y="0" Width="600" Height="400"><Column Header="Nom" Binding="{Binding Text}"/><Column Header="Taille" Binding="{Binding Size}"/></ListView></Panel>"#,
    );
    report("ListBox", r#"<Panel DesignWidth="600" DesignHeight="400"><ListBox ItemsSource="{Binding Items}" X="0" Y="0" Width="600" Height="400"/></Panel>"#);
    report(
        "DataTable",
        r#"<Panel DesignWidth="600" DesignHeight="400"><DataTable ItemsSource="{Binding Items}" X="0" Y="0" Width="600" Height="400"><Column Header="Nom" Binding="{Binding Text}"/><Column Header="Taille" Binding="{Binding Size}"/></DataTable></Panel>"#,
    );
    report(
        "Repeater (ItemHeight)",
        r#"<Panel DesignWidth="600" DesignHeight="400"><Repeater ItemsSource="{Binding Items}" ItemHeight="28" X="0" Y="0" Width="600" Height="400"><Stack Direction="LeftToRight"><Label Text="{Binding Text}"/><Label Text="{Binding Size}"/></Stack></Repeater></Panel>"#,
    );
    report(
        "Repeater (measured)",
        r#"<Panel DesignWidth="600" DesignHeight="400"><Repeater ItemsSource="{Binding Items}" X="0" Y="0" Width="600" Height="400"><Stack Direction="LeftToRight"><Label Text="{Binding Text}"/><Label Text="{Binding Size}"/></Stack></Repeater></Panel>"#,
    );
}
