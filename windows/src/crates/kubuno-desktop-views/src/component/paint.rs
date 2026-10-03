//! Painting a control (EVT-8): what a WinForms control's `WM_PAINT` does, for the hosts that paint
//! classes (`crate::node::custom::CustomControlNode`, [`super::ControlHost`]).
//!
//! [`paint_control`] runs, in order and as the control's [`ControlStyles`] say:
//!
//! - nothing at all when `USER_PAINT` is cleared (the control's look is not its own `on_paint`: a
//!   built-in node paints it);
//! - `on_paint_background` (the `BackColor`/`BackgroundImage` by default) unless `OPAQUE` —
//!   always in the paint pass, as with `ALL_PAINTING_IN_WM_PAINT` (Kubuno has no separate erase);
//! - `on_paint`, whose base behaviour raises `Paint` with the surface lent to the handlers.
//!
//! **Double buffering.** Every Kubuno frame is composed off screen (Direct2D, swap chain), so no
//! control ever flickers. On top of that, a control with `OPTIMIZED_DOUBLE_BUFFER` (the default,
//! WinForms' `DoubleBuffered`) keeps its paint in a **buffer** — the display list its `Graphics`
//! recorded — and the next frames replay the buffer instead of calling its paint methods again, as
//! long as the control stays **valid**: no `invalidate()`/`refresh()`, the same bounds, interaction
//! state, DPI scale and theme, and no property change (the view runtime invalidates a control whose
//! bound properties changed; [`super::ControlHost`] one it lent mutably). A paint that drew on the raw
//! canvas (`e.canvas()`), or whose `Paint` event has handlers (they may draw anything), is not kept.
//! `set_style(OPTIMIZED_DOUBLE_BUFFER, false)` (or `set_double_buffered(false)`) repaints every frame.

use kubuno_desktop_controls::host::paint_debug;
use kubuno_desktop_controls::ControlCanvas;
use kubuno_desktop_ui::graphics::{DisplayList, Graphics};
use kubuno_desktop_ui::{Rect, WidgetState};

use super::control::{Control, ControlStyles};
use super::cx::{PaintEventCx, RaiseSink};

/// What a buffered paint depends on besides the control's own state.
#[derive(Clone, PartialEq)]
struct BufferKey {
    bounds: Rect,
    state: WidgetState,
    scale: f32,
    /// A fingerprint of the theme (light/dark, accent).
    theme: [u32; 3],
}

/// A control's paint buffer (see the module doc).
#[derive(Default)]
pub struct PaintBuffer {
    list: Option<DisplayList>,
    key: Option<BufferKey>,
    /// How many times the paint methods ran (not replayed) — diagnostics and tests.
    pub repaints: u64,
    /// How many frames replayed the buffer.
    pub replays: u64,
}

impl PaintBuffer {
    /// Forgets the buffer (the next frame repaints).
    pub fn clear(&mut self) {
        self.list = None;
        self.key = None;
    }

    /// Whether a buffer is kept.
    pub fn is_filled(&self) -> bool {
        self.list.is_some()
    }

    /// The recorded paint, if kept.
    pub fn display_list(&self) -> Option<&DisplayList> {
        self.list.as_ref()
    }
}

fn theme_key(canvas: &dyn ControlCanvas) -> [u32; 3] {
    let t = canvas.theme();
    let c = |v: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F| (v.r * 255.0) as u32 | ((v.g * 255.0) as u32) << 8 | ((v.b * 255.0) as u32) << 16;
    [c(&t.window_background), c(&t.accent), c(&t.text_primary)]
}

/// How a paint went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintOutcome {
    /// `USER_PAINT` is cleared: nothing was painted.
    NotUserPaint,
    /// The paint methods ran.
    Painted,
    /// The buffer was replayed.
    Replayed,
}

/// Paints `control` into `bounds` on `canvas` (see the module doc). `sink` receives the `Paint`
/// event (the element's `.kbview` handler); `sink_handles_paint` says whether it has one (a buffer
/// is not kept then).
pub fn paint_control(control: &mut dyn Control, canvas: &dyn ControlCanvas, bounds: Rect, state: WidgetState, sink: Option<&mut dyn RaiseSink>, sink_handles_paint: bool) -> PaintOutcome {
    let styles = control.control_core().styles;
    if !styles.contains(ControlStyles::USER_PAINT) {
        let core = control.control_core_mut();
        core.invalid = None;
        core.update_requested = false;
        core.buffer.clear();
        return PaintOutcome::NotUserPaint;
    }
    let buffered = styles.contains(ControlStyles::OPTIMIZED_DOUBLE_BUFFER) && !sink_handles_paint && !control.component_core().events.has_subscribers("OnPaint");
    let key = BufferKey { bounds, state, scale: canvas.scale(), theme: theme_key(canvas) };
    // Validated before the paint: an invalidation made WHILE painting (an animation) stands, and
    // the host asks for another frame for it.
    let invalid = control.control_core_mut().invalid.take();
    control.control_core_mut().update_requested = false;
    if buffered && invalid.is_none() {
        let core = control.control_core_mut();
        if core.buffer.key.as_ref() == Some(&key) {
            if let Some(list) = core.buffer.list.as_ref() {
                list.replay(&Graphics::new(canvas));
                core.buffer.replays += 1;
                return PaintOutcome::Replayed;
            }
        }
    }
    let g = if buffered { Graphics::new(canvas).recording() } else { Graphics::new(canvas) };
    {
        let mut e = PaintEventCx::new(&g, canvas, bounds, state);
        if let Some(sink) = sink {
            e = e.with_sink(sink);
        }
        paint_layers(control, &mut e);
    }
    let recorded = g.take_recording();
    let complete = !g.has_unrecorded_drawing();
    let core = control.control_core_mut();
    let was_filled = core.buffer.list.is_some();
    match recorded {
        Some(list) if buffered && complete => {
            core.buffer.list = Some(list);
            core.buffer.key = Some(key);
        }
        _ => core.buffer.clear(),
    }
    core.buffer.repaints += 1;
    // The paint debug overlay flashes what was repainted because it was invalidated (or because
    // what it depends on changed), not the controls that simply paint every frame.
    if invalid.is_some() || (buffered && was_filled) {
        paint_debug::note_invalidated(crate::common::to_client(invalid.unwrap_or(bounds)));
    }
    PaintOutcome::Painted
}

/// The two layers of a paint, background then foreground (WinForms `PaintWithErrorHandling` over
/// `PaintLayerBackground` / `PaintLayerForeground`): `on_paint_background` unless `OPAQUE`, then
/// `on_paint`. What the default [`Control::on_print`] runs, and the hosts' paint.
pub fn paint_layers(control: &mut dyn Control, e: &mut PaintEventCx<'_>) {
    if !control.control_core().styles.contains(ControlStyles::OPAQUE) {
        control.on_paint_background(&mut e.reborrow());
    }
    control.on_paint(e);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::HasControlCore;
    use crate::prelude::*;
    use kubuno_desktop_ui::graphics::testing::RecordingCanvas;
    use kubuno_desktop_ui::graphics::{Color, RectExt};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A custom control logging its paint methods and drawing with the Graphics.
    #[derive(Component, Default)]
    #[kubuno(extends = Control, overrides(Control))]
    struct Gauge {
        base: ControlCore,
        value: f32,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Control for Gauge {
        fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>) {
            self.log.borrow_mut().push("background");
            self.base_mut().on_paint_background(e);
        }
        fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
            self.log.borrow_mut().push("paint");
            let r = e.bounds();
            e.graphics.fill_pie(Color::BLUE, r, -90.0, self.value * 3.6);
            self.base_mut().on_paint(e);
        }
    }

    fn gauge() -> (Gauge, Rc<RefCell<Vec<&'static str>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut g = Gauge { log: log.clone(), value: 25.0, ..Default::default() };
        g.set_bounds(Rect::from_xywh(10.0, 10.0, 100.0, 100.0));
        (g, log)
    }

    #[test]
    fn background_then_paint_and_opaque_skips_the_background() {
        let canvas = RecordingCanvas::new();
        let (mut g, log) = gauge();
        g.set_double_buffered(false);
        let b = g.bounds();
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Painted);
        assert_eq!(*log.borrow(), ["background", "paint"]);
        log.borrow_mut().clear();
        g.set_style(ControlStyles::OPAQUE, true);
        paint_control(&mut g, &canvas, b, WidgetState::REST, None, false);
        assert_eq!(*log.borrow(), ["paint"], "OPAQUE: no background pass");
        log.borrow_mut().clear();
        g.set_style(ControlStyles::USER_PAINT, false);
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST, None, false), PaintOutcome::NotUserPaint);
        assert!(log.borrow().is_empty(), "USER_PAINT cleared: the control's paint methods are not called");
    }

    #[test]
    fn a_double_buffered_control_replays_until_invalidated() {
        let canvas = RecordingCanvas::new();
        let (mut g, log) = gauge();
        assert!(g.double_buffered(), "double buffered by default");
        let b = g.bounds();
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Painted);
        let first = canvas.calls();
        canvas.clear();
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Replayed);
        assert_eq!(canvas.calls(), first, "the replay draws exactly what the paint drew");
        assert_eq!(*log.borrow(), ["background", "paint"], "the paint methods ran once");

        // Invalidation, a new state, new bounds: each repaints once.
        g.value = 50.0;
        g.invalidate();
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Painted);
        assert_eq!(paint_control(&mut g, &canvas, b, WidgetState::REST.hot(true), None, false), PaintOutcome::Painted);
        let moved = b.offset(5.0, 0.0);
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, false), PaintOutcome::Painted);
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, false), PaintOutcome::Replayed);
        assert_eq!(g.control_core().buffer.repaints, 4);
        assert!(g.invalidated_rect().is_none(), "painting validates the control");

        // Not buffered: a Paint handler, a Rust subscriber, or DoubleBuffered off.
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, true), PaintOutcome::Painted);
        let sub = g.paint().subscribe(|_, _| {});
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, false), PaintOutcome::Painted);
        drop(sub);
        g.set_double_buffered(false);
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, false), PaintOutcome::Painted);
        assert_eq!(paint_control(&mut g, &canvas, moved, WidgetState::REST.hot(true), None, false), PaintOutcome::Painted);
    }

    /// Drawing on the raw canvas cannot be replayed: the buffer is not kept.
    #[derive(Component, Default)]
    #[kubuno(extends = Control, overrides(Control))]
    struct Raw {
        base: ControlCore,
    }

    impl Control for Raw {
        fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
            let c = e.canvas();
            c.fill_rect(&e.bounds(), &c.theme().accent);
        }
    }

    #[test]
    fn a_paint_on_the_raw_canvas_is_repainted_every_frame() {
        let canvas = RecordingCanvas::new();
        let mut r = Raw::default();
        r.set_bounds(Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
        let b = r.bounds();
        assert_eq!(paint_control(&mut r, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Painted);
        assert_eq!(paint_control(&mut r, &canvas, b, WidgetState::REST, None, false), PaintOutcome::Painted);
        assert!(!r.control_core().buffer.is_filled());
    }

    #[test]
    fn the_default_background_paints_back_color_and_honours_transparency() {
        let canvas = RecordingCanvas::new();
        let (mut g, _) = gauge();
        g.set_double_buffered(false);
        let b = g.bounds();
        paint_control(&mut g, &canvas, b, WidgetState::REST, None, false);
        let plain = canvas.count("fill_rounded(10,10,110,110");
        assert_eq!(plain, 0, "no BackColor: the parent shows through");
        canvas.clear();
        g.control_core_mut().props.back_color = Some(Color::rgb(200, 0, 0).with_alpha(0.5).to_d2d());
        paint_control(&mut g, &canvas, b, WidgetState::REST, None, false);
        assert_eq!(canvas.count("fill_rounded(10,10,110,110"), 1, "no transparency support: the colour is made opaque (one fill)");
        canvas.clear();
        g.set_style(ControlStyles::SUPPORTS_TRANSPARENT_BACK_COLOR, true);
        paint_control(&mut g, &canvas, b, WidgetState::REST, None, false);
        assert_eq!(canvas.count("fill_rounded(10,10,110,110"), 2, "transparent: the parent's background, then the colour over it");
    }

    #[test]
    fn on_print_paints_background_then_foreground_and_draws_to_a_display_list() {
        let (mut g, log) = gauge();
        let list = g.draw_to_display_list();
        assert_eq!(*log.borrow(), ["background", "paint"], "on_print = background + paint, no event of its own");
        // Shifted to the control's origin: the whole list runs inside an offset of -bounds.
        let names = list.describe();
        assert_eq!(names.first().map(String::as_str), Some("Canvas.PushOffset"));
        assert_eq!(names.last().map(String::as_str), Some("Canvas.PopOffset"));
        assert!(names.iter().any(|n| n == "FillPath"), "the pie: {names:?}");
        // Replayed into another surface at an offset.
        let canvas = RecordingCanvas::new();
        let target = Graphics::new(&canvas);
        g.draw_to_bitmap(&target, Rect::from_xywh(300.0, 0.0, 100.0, 100.0));
        assert!(canvas.calls().iter().any(|c| c.starts_with("fill_triangle")), "the pie was drawn (fallback surface)");
    }

    #[test]
    fn invoke_paint_reaches_a_child_control() {
        let canvas = RecordingCanvas::new();
        let (mut parent, _) = gauge();
        let (mut child, log) = gauge();
        let g = Graphics::new(&canvas);
        let mut e = PaintEventCx::new(&g, &canvas, child.bounds(), WidgetState::REST);
        parent.invoke_paint_background(&mut child, &mut e);
        parent.invoke_paint(&mut child, &mut e);
        assert_eq!(*log.borrow(), ["background", "paint"]);
    }

}
