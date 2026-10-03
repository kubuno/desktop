//! The `Graphics` API against a recorder and a recording canvas.

use super::testing::RecordingCanvas;
use super::*;

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_xywh(x, y, w, h)
}

#[test]
fn every_call_is_an_op_with_its_state() {
    let g = Graphics::recorder();
    let pen = Pen::new(Color::BLACK, 2.0).with_dash(DashStyle::DashDot).with_caps(LineCap::Round);
    g.draw_line(&pen, PointF::new(0.0, 0.0), PointF::new(10.0, 10.0));
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    g.draw_rounded_rectangle(&pen, r(0.0, 0.0, 10.0, 10.0), 3.0);
    g.fill_ellipse(RadialGradientBrush::from_rect(r(0.0, 0.0, 10.0, 10.0), Color::WHITE, Color::BLACK), r(0.0, 0.0, 10.0, 10.0));
    g.draw_arc(&pen, r(0.0, 0.0, 10.0, 10.0), 135.0, 270.0);
    g.fill_pie(Color::BLUE, r(0.0, 0.0, 10.0, 10.0), 0.0, 90.0);
    g.draw_bezier(&pen, PointF::new(0.0, 0.0), PointF::new(1.0, 5.0), PointF::new(5.0, 1.0), PointF::new(6.0, 6.0));
    g.fill_polygon(Color::GREEN, &[PointF::new(0.0, 0.0), PointF::new(5.0, 0.0), PointF::new(0.0, 5.0)]);
    g.draw_curve(&pen, &[PointF::new(0.0, 0.0), PointF::new(5.0, 5.0), PointF::new(10.0, 0.0)], 0.5);
    g.draw_string("Hi", &Font::default(), Color::BLACK, r(0.0, 0.0, 50.0, 20.0), &StringFormat::centered());
    g.draw_image(&Image::from_file("C:\\none.png"), r(0.0, 0.0, 16.0, 16.0));
    g.draw_icon("folder", r(0.0, 0.0, 16.0, 16.0), 16.0, Color::BLACK);
    g.clear(Color::TRANSPARENT);
    let list = g.take_recording().expect("recording");
    assert_eq!(
        list.describe(),
        vec![
            "StrokeLine",
            "FillRect",
            "StrokeRoundedRect",
            "FillEllipse",
            "StrokePath",
            "FillPath",
            "StrokePath",
            "FillPath",
            "StrokePath",
            "Text(\"Hi\")",
            "Image",
            "Icon(folder)",
            "Clear"
        ]
    );
    match &list.ops[0] {
        Op::Stroke { pen, state, .. } => {
            assert_eq!(pen.effective_pattern(), &[3.0, 1.0, 1.0, 1.0]);
            assert_eq!(pen.start_cap, LineCap::Round);
            assert!(state.transform.is_identity() && state.clips.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(g.take_recording().is_none(), "taking the recording stops it");
}

#[test]
fn invisible_brushes_pens_and_empty_text_draw_nothing() {
    let g = Graphics::recorder();
    g.fill_rectangle(Color::TRANSPARENT, r(0.0, 0.0, 1.0, 1.0));
    g.draw_rectangle(&Pen::new(Color::BLACK, 0.0), r(0.0, 0.0, 1.0, 1.0));
    g.draw_string("", &Font::default(), Color::BLACK, r(0.0, 0.0, 1.0, 1.0), &StringFormat::default());
    g.draw_path(&Pen::new(Color::BLACK, 1.0), &GraphicsPath::new());
    g.draw_image_with(&Image::from_file("x.png"), r(0.0, 0.0, 1.0, 1.0), None, 0.0);
    assert!(g.recorded().is_some_and(|l| l.is_empty()));
}

#[test]
fn transforms_prepend_and_save_restore_nests() {
    let g = Graphics::recorder();
    g.translate_transform(100.0, 0.0);
    let outer = g.save();
    g.scale_transform(2.0, 2.0);
    g.set_smoothing_mode(SmoothingMode::None);
    let inner = g.save();
    g.rotate_transform(90.0);
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 1.0, 1.0));
    g.restore(inner);
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 1.0, 1.0));
    g.restore(outer);
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 1.0, 1.0));
    g.restore(inner); // already discarded: nothing happens
    let list = g.take_recording().expect("recording");
    let states: Vec<&OpState> = list.ops.iter().filter_map(Op::state).collect();
    // Rotated, then scaled, in the translated space: (1, 0) → rot (0, 1) → scale (0, 2) → +100.
    let p = states[0].transform.transform_point(PointF::new(1.0, 0.0));
    assert!((p.x - 100.0).abs() < 1e-4 && (p.y - 2.0).abs() < 1e-4);
    assert_eq!(states[0].smoothing, SmoothingMode::None);
    assert_eq!(states[1].transform.transform_point(PointF::new(1.0, 1.0)), PointF::new(102.0, 2.0));
    assert_eq!(states[2].transform, Matrix::translation(100.0, 0.0));
    assert_eq!(states[2].smoothing, SmoothingMode::Default);
    let n = g.with_saved(|g| {
        g.reset_transform();
        7
    });
    assert_eq!((n, g.transform()), (7, Matrix::translation(100.0, 0.0)));
}

#[test]
fn clips_intersect_replace_and_follow_the_transform() {
    let g = Graphics::recorder();
    assert!(g.clip_bounds().is_none() && !g.is_clipped());
    g.set_clip(r(0.0, 0.0, 100.0, 100.0));
    g.translate_transform(50.0, 50.0);
    g.intersect_clip(r(0.0, 0.0, 100.0, 100.0));
    // In surface space the clip is 50..100; in the translated coordinates, 0..50.
    let b = g.clip_bounds().expect("clipped");
    assert_eq!((b.left, b.top, b.right, b.bottom), (0.0, 0.0, 50.0, 50.0));
    assert!(g.is_visible(r(10.0, 10.0, 5.0, 5.0)));
    assert!(!g.is_visible(r(60.0, 60.0, 5.0, 5.0)));
    let s = g.save();
    let mut circle = GraphicsPath::new();
    circle.add_ellipse(r(0.0, 0.0, 10.0, 10.0));
    g.set_clip_path(&circle);
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    g.restore(s);
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    g.reset_clip();
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    let list = g.take_recording().expect("recording");
    let clips: Vec<usize> = list.ops.iter().filter_map(Op::state).map(|s| s.clips.len()).collect();
    assert_eq!(clips, vec![1, 2, 0], "set_clip replaces, restore brings both back, reset clears");
    match &list.ops[0].state().expect("state").clips[0].shape {
        ClipShape::Path(_) => {}
        other => panic!("unexpected {other:?}"),
    }
    // Op bounds are cut by the clip: the second fill (at 50,50 in the surface) inside 50..100.
    let b = list.ops[1].bounds().expect("bounds");
    assert_eq!((b.left, b.top, b.right, b.bottom), (50.0, 50.0, 60.0, 60.0));
}

#[test]
fn text_is_measured_and_placed() {
    let g = Graphics::recorder();
    let font = Font::default().sized(10.0);
    let s = g.measure_string("abcd", &font, None, &StringFormat::default());
    assert!((s.width - 22.0).abs() < 1e-4, "the recorder's deterministic measure");
    let wrapped = g.measure_string("abcd", &font, Some(11.0), &StringFormat::default());
    assert!(wrapped.height > s.height);
    g.draw_string_at("abcd", &font, Color::BLACK, PointF::new(5.0, 6.0));
    let list = g.take_recording().expect("recording");
    match &list.ops[0] {
        Op::Text { layout, format, .. } => {
            assert_eq!((layout.left, layout.top), (5.0, 6.0));
            assert!(format.flags.contains(StringFormatFlags::NO_CLIP) && !format.wraps());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn canvas_primitives_called_on_a_graphics_are_recorded_and_forwarded() {
    let canvas = RecordingCanvas::new();
    let g = Graphics::new(&canvas).recording();
    assert!(!g.has_device(), "a recording canvas lends no renderer: fallback mode");
    let theme = g.theme().clone();
    g.fill_rounded(&r(0.0, 0.0, 10.0, 10.0), 2.0, &theme.accent);
    g.push_clip(&r(0.0, 0.0, 5.0, 5.0));
    g.text_ellipsis("x", &r(0.0, 0.0, 5.0, 5.0), &g.formats().body, &theme.text_primary);
    g.pop_clip();
    assert_eq!(canvas.calls(), vec!["fill_rounded(0,0,10,10 r=2)", "push_clip(0,0,5,5)", "text(\"x\" 0,0,5,5)", "pop_clip"]);
    assert_eq!(g.recorded().map(|l| l.len()), Some(4));
    assert!(!g.has_unrecorded_drawing());
    let _ = g.raw_canvas();
    assert!(g.has_unrecorded_drawing());
}

#[test]
fn the_fallback_maps_shapes_onto_primitives() {
    let canvas = RecordingCanvas::new();
    let g = Graphics::new(&canvas);
    g.fill_rectangle(Color::RED, r(10.0, 10.0, 20.0, 10.0));
    g.fill_ellipse(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    g.draw_ellipse(&Pen::new(Color::RED, 2.0), r(0.0, 0.0, 10.0, 10.0));
    g.draw_rectangle(&Pen::new(Color::RED, 1.0).with_alignment(PenAlignment::Inset), r(0.0, 0.0, 10.0, 10.0));
    g.draw_line(&Pen::new(Color::RED, 2.0), PointF::new(0.0, 5.0), PointF::new(10.0, 5.0));
    g.translate_transform(100.0, 0.0);
    g.set_clip(r(0.0, 0.0, 50.0, 50.0));
    g.fill_rectangle(LinearGradientBrush::new(PointF::new(0.0, 0.0), PointF::new(1.0, 0.0), Color::BLACK, Color::WHITE), r(0.0, 0.0, 5.0, 5.0));
    g.draw_string("t", &Font::default(), Color::BLACK, r(0.0, 0.0, 20.0, 10.0), &StringFormat::centered());
    g.fill_polygon(Color::RED, &[PointF::new(0.0, 0.0), PointF::new(4.0, 0.0), PointF::new(4.0, 4.0), PointF::new(0.0, 4.0)]);
    let calls = canvas.calls();
    assert_eq!(calls[0], "fill_rounded(10,10,30,20 r=0)");
    assert_eq!(calls[1], "fill_rounded(0,0,10,10 r=5)", "a circle is a fully rounded box");
    assert_eq!(calls[2], "stroke_arc(5,5 r=5 w=2)");
    assert_eq!(calls[3], "stroke_rounded_w(0.5,0.5,9.5,9.5 r=0 w=1)", "an inset pen stays inside");
    assert_eq!(calls[4], "fill_rounded(0,4,10,6 r=0)", "a horizontal line is a thin bar");
    assert_eq!(&calls[5..8], &["push_clip(100,0,150,50)", "fill_rounded(100,0,105,5 r=0)", "pop_clip"]);
    assert_eq!(&calls[8..11], &["push_clip(100,0,150,50)", "text(\"t\" 100,0,120,10)", "pop_clip"]);
    assert!(calls[12..].iter().filter(|c| c.as_str() == "fill_triangle").count() >= 2, "a polygon is a triangle fan");
}

#[test]
fn a_display_list_replays_in_its_recorded_state() {
    let g = Graphics::recorder();
    g.translate_transform(10.0, 0.0);
    g.set_clip(r(0.0, 0.0, 5.0, 5.0));
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 10.0, 10.0));
    g.fill_rounded(&r(0.0, 0.0, 1.0, 1.0), 0.0, &Color::BLUE.to_d2d());
    let list = g.take_recording().expect("recording");
    let bounds = list.bounds().expect("bounds");
    assert_eq!((bounds.left, bounds.top, bounds.right, bounds.bottom), (10.0, 0.0, 15.0, 5.0));

    // Replayed on a surface in another state: the recorded one applies.
    let canvas = RecordingCanvas::new();
    let g2 = Graphics::new(&canvas).recording();
    g2.scale_transform(3.0, 3.0);
    list.replay(&g2);
    assert_eq!(canvas.calls(), vec!["push_clip(10,0,15,5)", "fill_rounded(10,0,20,10 r=0)", "pop_clip", "fill_rounded(0,0,1,1 r=0)"]);
    assert_eq!(g2.recorded().map(|l| l.len()), Some(2), "replayed ops are recorded again");
}

#[test]
fn a_null_graphics_draws_and_records_nothing_but_answers() {
    let g = Graphics::null();
    assert!(g.is_null() && !g.has_device());
    g.fill_rectangle(Color::RED, r(0.0, 0.0, 1.0, 1.0));
    assert!(g.recorded().is_none());
    assert!(g.measure_string("ab", &Font::default(), None, &StringFormat::default()).width > 0.0);
    // The canvas face still answers (headless theme and formats).
    let _ = g.theme().accent;
    assert!(g.measure("ab", &g.formats().body) > 0.0);
}

// -- Owner-draw through the real widgets ------------------------------------------------------

fn owner_list(mode: DrawMode) -> crate::lists::ListBox {
    let mut list = crate::lists::ListBox::new();
    for s in ["alpha", "beta", "gamma"] {
        list.items.push(s.to_string());
    }
    list.draw_mode = mode;
    list.set_selected_index(1);
    list
}

#[test]
fn an_owner_drawn_list_box_offers_each_row_to_the_handler() {
    use crate::Widget;
    let canvas = RecordingCanvas::new();
    let list = owner_list(DrawMode::OwnerDrawFixed);
    let bounds = r(0.0, 0.0, 200.0, 200.0);
    let mut seen = Vec::new();
    owner_draw::with_handler(
        &mut |e: &mut DrawItemEventArgs<'_>| {
            seen.push((e.index, e.text.clone(), e.state.contains(DrawItemState::SELECTED), e.bounds.top));
            // Row 2 is left to the list.
            e.draw_default = e.index == Some(2);
            if !e.draw_default {
                e.graphics.draw_icon("folder", e.bounds, 16.0, e.fore_color);
            }
        },
        || list.paint(&canvas, bounds, crate::WidgetState::REST),
    );
    assert_eq!(seen.len(), 3);
    assert_eq!((seen[1].1.as_str(), seen[1].2), ("beta", true), "the selected row says so");
    assert!(seen[0].3 < seen[1].3 && seen[1].3 < seen[2].3, "in row order, top to bottom");
    let calls = canvas.calls();
    assert_eq!(calls.iter().filter(|c| c.starts_with("icon(folder")).count(), 2);
    assert!(calls.iter().any(|c| c.starts_with("text(\"gamma\"")), "the default row keeps its label");
    assert!(!calls.iter().any(|c| c.starts_with("text(\"alpha\"")), "an owner-drawn row gets no default label");

    // Without a handler the list paints itself.
    canvas.clear();
    list.paint(&canvas, bounds, crate::WidgetState::REST);
    assert!(canvas.calls().iter().any(|c| c.starts_with("text(\"alpha\"")));
}

struct Variable;
impl OwnerDrawHandler for Variable {
    fn measure_item(&mut self, e: &mut MeasureItemEventArgs<'_>) {
        e.item_height = 20.0 + 20.0 * e.index as f32;
    }
    fn draw_item(&mut self, e: &mut DrawItemEventArgs<'_>) {
        e.draw_text();
    }
}

#[test]
fn an_owner_draw_variable_list_box_measures_then_lays_out_by_height() {
    use crate::Widget;
    let canvas = RecordingCanvas::new();
    let mut list = owner_list(DrawMode::OwnerDrawVariable);
    let bounds = r(0.0, 0.0, 200.0, 70.0);
    owner_draw::with_handler(&mut Variable, || list.measure_items(&canvas, bounds));
    assert_eq!(list.item_heights.as_deref(), Some(&[20.0, 40.0, 60.0][..]));
    // Rows are stacked by their own heights (content starts inside the panel's border + padding).
    let a = list.item_rect(bounds, 0).expect("row 0");
    let b = list.item_rect(bounds, 1).expect("row 1");
    assert_eq!((b.top - a.top, a.bottom - a.top, b.bottom - b.top), (20.0, 20.0, 40.0));
    assert_eq!(list.item_at(bounds, 50.0, b.top + 30.0), Some(1));
    assert!(list.item_rect(bounds, 2).is_none_or(|c| c.top >= b.bottom), "row 2 is below row 1");
    assert_eq!(list.visible_rows(bounds), 2, "20 + 40 fit in the content, 60 more do not");
    assert_eq!(list.max_top_index(bounds), 2);
    // Painting inside the same scope uses the measured rows.
    owner_draw::with_handler(&mut Variable, || list.paint(&canvas, bounds, crate::WidgetState::REST));
    assert!(canvas.calls().iter().any(|c| c.starts_with("text(\"beta\"")));
    // Back to fixed: the heights no longer apply.
    list.draw_mode = DrawMode::OwnerDrawFixed;
    owner_draw::with_handler(&mut Variable, || list.measure_items(&canvas, bounds));
    assert!(list.item_heights.is_none());
}
