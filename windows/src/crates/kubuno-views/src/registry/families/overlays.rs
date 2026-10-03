//! Component family `overlays` — `<Popover>`, a floating panel anchored to a control of the view
//! (the web's `AnchoredPopover`, `kubuno_ui::dialogs::Popover`): a picker, a small form, a
//! user card. Compiled with the `family-overlays` feature (on by default through
//! `all-families`).
//!
//! It opens when `IsOpen` becomes true (a binding, or `popover.show()` from code), next to its
//! `Target` (the `x:Name` of a control) on the preferred `Placement` side when it fits, and it
//! is light-dismissed: a press outside it or Escape closes it (`IsOpen`
//! is written back, `OnClosed` raised); that press only closes it, the view under it does not get
//! it. Wherever it is declared, it is painted above the whole view, after it
//! (`crate::node::TopLayerRoot`). A whole view shown as a flyout
//! window is the `WindowKind="Flyout"` window kind instead.
//!
//! In the designer it is shown open in its own box (its content is edited there).

#[allow(unused_imports)] // Used by the `component!` invocation below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use crate::binding::{PropSource, Value, ViewModel};
use crate::node::{PaintCx, ViewEventKind, ViewNode};

use kubuno_controls::host::{self, vk, Modifiers};
use kubuno_ui::dialogs::{place_anchored_in, Align};
use kubuno_ui::display::Side;
use kubuno_ui::{Canvas, Rect, Size, Widget, WidgetState};

component! {
    mod_name: popover,
    name: "Popover",
    // Note: `kubuno_ui::dialogs::Popover` anchored to another element; see this family's doc.
    doc: "A floating panel shown next to a control (its Target) while IsOpen is true; a click outside it or Escape closes it. It is painted above the rest of the view wherever it is declared.",
    ctor: kubuno_ui::dialogs::Popover::new(),
    children: ChildrenModel::SingleWidget,
    default_event: "OnClosed",
    props: [
        PropertyMeta::new("Target", PropKind::String, "", "The x:Name of the control it opens next to.").category("Behavior").editor("reference:Control"),
        PropertyMeta::new("IsOpen", PropKind::Bool, "false", "Whether it is shown. Bind it two-way to know when the user closes it.").category("Behavior").bindable(),
        PropertyMeta::new("Placement", PropKind::Enum(&["Bottom", "Top", "Left", "Right"]), "Bottom", "The side of the target it opens on, when there is room.").category("Layout"),
        PropertyMeta::new("Alignment", PropKind::Enum(&["Start", "Center", "End"]), "Start", "Which edges of the panel and the target line up.").category("Layout"),
        PropertyMeta::new("PopupWidth", PropKind::F32, "0", "The width of the panel, in DIP; 0 for its content's.").category("Layout"),
        PropertyMeta::new("PopupHeight", PropKind::F32, "0", "The height of the panel, in DIP; 0 for its content's.").category("Layout"),
        PropertyMeta::new("LightDismiss", PropKind::Bool, "true", "Closes it on a click outside it or Escape.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnOpened", "Occurs when the panel opens.").category(crate::registry::EventCategory::Behavior),
        EventMeta::new("OnClosed", "Occurs when the panel closes.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |p| { p.side(kubuno_ui::display::Side::Bottom).align(kubuno_ui::dialogs::Align::Start) },
    build: |props, cx| {
        let target = match props.str("Target", "")? {
            crate::binding::PropSource::Literal(s) => s,
            crate::binding::PropSource::Bound { .. } => return Err(BuildError::new("attribute `Target` must be a literal value, not a binding", props.element().name_range())),
        };
        let node = crate::registry::families::overlays::PopoverNode {
            target,
            open: props.bool("IsOpen", false)?,
            placement: props.enum_("Placement", "Bottom")?,
            alignment: props.enum_("Alignment", "Start")?,
            width: props.f32("PopupWidth", 0.0)?,
            height: props.f32("PopupHeight", 0.0)?,
            light_dismiss: props.bool("LightDismiss", true)?,
            on_opened: props.event("OnOpened"),
            on_closed: props.event("OnClosed"),
            focus_id: props.focus_id(),
            child: props.build_single_child(cx)?,
            was_open: false,
            pressed: false,
            closed_here: false,
            home: kubuno_ui::Rect::default(),
            shown: None,
            dismissing: false,
            swallowing: false,
        };
        // Painted above the whole view, wherever it is declared; its place in the document only
        // holds it in the designer.
        let shared = std::rc::Rc::new(std::cell::RefCell::new(node));
        cx.top_layer.push(shared.clone());
        Ok(Box::new(crate::registry::families::overlays::PopoverSlot { shared }) as Box<dyn ViewNode>)
    },
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[popover::META];

/// `<Popover>`'s live node.
pub struct PopoverNode {
    target: String,
    open: PropSource<bool>,
    placement: PropSource<String>,
    alignment: PropSource<String>,
    width: PropSource<f32>,
    height: PropSource<f32>,
    light_dismiss: PropSource<bool>,
    on_opened: Option<String>,
    on_closed: Option<String>,
    focus_id: Option<kubuno_ui::FocusId>,
    child: Option<Box<dyn ViewNode>>,
    was_open: bool,
    /// The mouse was down at the last frame (a press is its down edge).
    pressed: bool,
    /// It closed itself (light dismiss) while `IsOpen` is still true in a one-way source: it stays
    /// closed until `IsOpen` goes false then true again.
    closed_here: bool,
    /// Where it is declared (its slot's box last frame): the anchor of a panel without a `Target`.
    home: Rect,
    /// The panel's box last frame, while it is shown.
    shown: Option<Rect>,
    /// It is light-dismissed: while shown, it keeps every press (the view under it sees none).
    dismissing: bool,
    /// It was just closed by a press outside it: that press (until its release) is kept from the view.
    swallowing: bool,
}

impl PopoverNode {
    /// The panel's size: the set one, else its content's.
    fn size(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let content = self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::new(232.0, 120.0));
        let w = self.width.resolve(vm);
        let h = self.height.resolve(vm);
        Size::new(if w > 0.0 { w } else { content.width.max(120.0) }, if h > 0.0 { h } else { content.height.max(40.0) })
    }

    fn paint_panel(&mut self, cx: &mut PaintCx<'_>, rect: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        let side = side_of(&self.placement.resolve(&*cx.vm));
        let panel = kubuno_ui::dialogs::Popover::sized(Size::new(rect.right - rect.left, rect.bottom - rect.top)).side(side);
        panel.paint(canvas, rect, WidgetState::REST);
        if let Some(child) = self.child.as_mut() {
            let mut inner = cx.reborrow();
            inner.canvas.push_clip_rounded(&rect, kubuno_ui::metrics::radius::FLOAT);
            child.paint(&mut inner, rect);
            inner.canvas.pop_clip_rounded();
        }
    }

    fn close(&mut self, cx: &mut PaintCx<'_>) {
        self.closed_here = true;
        if let Some(spec) = self.open.binding().filter(|s| s.mode.writes_back()) {
            spec.update_source(cx.vm, Value::Bool(false));
        }
        self.was_open = false;
        cx.fire("OnClosed", self.focus_id, self.on_closed.as_deref(), ViewEventKind::Clicked, &mut crate::events::EmptyEventArgs);
        host::request_repaint_after(0);
    }
}

fn side_of(s: &str) -> Side {
    match s {
        "Top" => Side::Top,
        "Left" => Side::Left,
        "Right" => Side::Right,
        _ => Side::Bottom,
    }
}

/// `<Popover>`'s place in the document: it holds the panel in the designer (edited in place) and
/// remembers where it is declared (the anchor of a panel without a `Target`); at run time the
/// panel itself is painted above the whole view ([`crate::node::TopLayerRoot`]).
pub struct PopoverSlot {
    pub(crate) shared: std::rc::Rc<std::cell::RefCell<PopoverNode>>,
}

impl ViewNode for PopoverSlot {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        // It takes room in its container only in the designer, where it is shown open.
        if !crate::common::design_frame() {
            return Size::new(0.0, 0.0);
        }
        self.shared.try_borrow().map(|n| n.size(c, vm)).unwrap_or(Size::new(0.0, 0.0))
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let Ok(mut node) = self.shared.try_borrow_mut() else { return };
        if cx.design.is_some() {
            // Edited in place, open, in its own box.
            let size = node.size(cx.canvas, &*cx.vm);
            let rect = Rect::new(bounds.left, bounds.top, bounds.left + size.width.min(bounds.right - bounds.left).max(40.0), bounds.top + size.height.min(bounds.bottom - bounds.top).max(24.0));
            node.paint_panel(cx, rect);
            return;
        }
        node.home = bounds;
    }
}

impl crate::node::TopLayer for PopoverNode {
    fn hold(&self) -> Option<Option<Rect>> {
        if self.swallowing {
            return Some(None);
        }
        let rect = self.shown?;
        Some(if self.dismissing { None } else { Some(rect) })
    }

    fn paint_top(&mut self, cx: &mut PaintCx<'_>, _window: Rect) {
        // The press that closed it is kept from the view until its release.
        if self.swallowing && !cx.frame.mouse_down && !cx.frame.right_down {
            self.swallowing = false;
        }
        self.shown = None;
        let wanted = self.open.resolve(&*cx.vm);
        if !wanted {
            self.closed_here = false;
            if self.was_open {
                self.was_open = false;
                cx.fire("OnClosed", self.focus_id, self.on_closed.as_deref(), ViewEventKind::Clicked, &mut crate::events::EmptyEventArgs);
            }
            self.pressed = cx.frame.mouse_down;
            return;
        }
        if self.closed_here {
            self.pressed = cx.frame.mouse_down;
            return;
        }
        let size = self.size(cx.canvas, &*cx.vm);
        let target = cx.router.as_deref().and_then(|r| r.bounds_of(&self.target));
        let anchor = target.unwrap_or(Rect::new(self.home.left, self.home.top, self.home.left, self.home.top));
        let (w, h) = cx.frame.size;
        let area = Rect::new(0.0, 0.0, w, h);
        let align = match self.alignment.resolve(&*cx.vm).as_str() {
            "Center" => Align::Center,
            "End" => Align::End,
            _ => Align::Start,
        };
        let placed = place_anchored_in(anchor, size, side_of(&self.placement.resolve(&*cx.vm)), align, area);
        let rect = placed.rect;
        if !self.was_open {
            self.was_open = true;
            // The press that opened it (a click on the target) must not close it.
            self.pressed = cx.frame.mouse_down;
            cx.fire("OnOpened", self.focus_id, self.on_opened.as_deref(), ViewEventKind::Clicked, &mut crate::events::EmptyEventArgs);
        }
        self.paint_panel(cx, rect);
        self.shown = Some(rect);
        self.dismissing = self.light_dismiss.resolve(&*cx.vm);
        if !self.dismissing {
            self.pressed = cx.frame.mouse_down;
            return;
        }
        let (mx, my) = cx.frame.mouse;
        let down_edge = cx.frame.mouse_down && !self.pressed;
        self.pressed = cx.frame.mouse_down;
        // A press outside the panel only closes it: the view under it does not get it (the
        // runtime showed it the pointer away while the panel was open).
        let outside = down_edge && !rect.contains(mx, my);
        if outside || host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
            self.close(cx);
            self.shown = None;
            self.swallowing = outside;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::MapViewModel;
    use crate::runtime::Runtime;
    use kubuno_controls::host::Frame;
    use kubuno_ui::graphics::testing::RecordingCanvas;

    fn frame(mouse: Option<(f32, f32)>, down: bool) -> Frame {
        let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
        Frame {
            size: (400.0, 300.0),
            mouse: (x, y),
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
            click_count: 1,
            window_focused: true,
        }
    }

    const VIEW: &str = r#"<Panel DesignWidth="400" DesignHeight="300">
  <Button x:Name="anchor" Text="Ouvrir" X="10" Y="10" Width="100" Height="30"/>
  <Popover Target="anchor" IsOpen="{Binding Open, Mode=TwoWay}" PopupWidth="200" PopupHeight="80">
    <Label Text="Dans le popover"/>
  </Popover>
</Panel>"#;

    fn run(rt: &mut Runtime, vm: &mut MapViewModel, f: Frame) -> RecordingCanvas {
        let canvas = RecordingCanvas::new();
        let mut handlers = crate::binding::HandlerTable::new();
        rt.frame(&canvas, &f, vm, &mut handlers, Rect::new(0.0, 0.0, 400.0, 300.0));
        canvas
    }

    fn shows(c: &RecordingCanvas) -> Option<String> {
        c.calls().into_iter().find(|s| s.starts_with("text(\"Dans le popover\""))
    }

    #[test]
    fn it_opens_under_its_target_and_a_click_outside_closes_it() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Open", Value::Bool(false));
        assert!(shows(&run(&mut rt, &mut vm, frame(None, false))).is_none());
        vm.set("Open", Value::Bool(true));
        let c = run(&mut rt, &mut vm, frame(None, false));
        let text = shows(&c).expect("shown when open");
        // Under the button (bottom 40): the label's box starts below it.
        let top: f32 = text.split(' ').nth(3).and_then(|r| r.split(',').nth(1)).and_then(|v| v.parse().ok()).unwrap_or(-1.0);
        assert!(top >= 40.0, "{text}");
        run(&mut rt, &mut vm, frame(Some((390.0, 290.0)), true));
        assert_eq!(vm.get("Open"), Some(Value::Bool(false)), "light-dismissed");
        assert!(shows(&run(&mut rt, &mut vm, frame(None, false))).is_none());
    }
}

#[cfg(test)]
mod top_layer_tests {
    use super::*;
    use crate::binding::MapViewModel;
    use crate::runtime::Runtime;
    use kubuno_controls::host::Frame;
    use kubuno_ui::graphics::testing::RecordingCanvas;

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
            click_count: 1,
            window_focused: true,
        }
    }

    // Declared FIRST, under a button it overlaps, with another button elsewhere.
    const VIEW: &str = r#"<Panel DesignWidth="400" DesignHeight="300">
  <Popover Target="anchor" IsOpen="{Binding Open, Mode=TwoWay}" PopupWidth="200" PopupHeight="80">
    <Label Text="Dans le popover"/>
  </Popover>
  <Button x:Name="anchor" Text="Ouvrir" X="10" Y="10" Width="100" Height="30"/>
  <Button x:Name="under" Text="Dessous" X="10" Y="50" Width="150" Height="30" OnClick="under_click"/>
  <Button x:Name="far" Text="Loin" X="300" Y="250" Width="80" Height="30" OnClick="far_click"/>
</Panel>"#;

    #[test]
    fn it_paints_above_the_view_and_keeps_the_clicks_while_open() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Open", Value::Bool(true));
        let mut handlers = crate::binding::HandlerTable::new();
        let mut step = |rt: &mut Runtime, vm: &mut MapViewModel, f: Frame| {
            let canvas = RecordingCanvas::new();
            let events = rt.frame(&canvas, &f, vm, &mut handlers, Rect::new(0.0, 0.0, 400.0, 300.0));
            (canvas, events.into_iter().filter_map(|e| e.handler).collect::<Vec<_>>())
        };
        let (c, _) = step(&mut rt, &mut vm, frame((host::POINTER_AWAY, host::POINTER_AWAY), false));
        let calls = c.calls();
        let panel = calls.iter().position(|s| s.starts_with("text(\"Dans le popover\"")).expect("shown");
        let under = calls.iter().position(|s| s.starts_with("text(\"Dessous\"")).expect("button painted");
        assert!(panel > under, "the panel paints after (above) the button declared after it");
        // A click on the button under the panel: the panel keeps it.
        let mut seen = step(&mut rt, &mut vm, frame((60.0, 60.0), false)).1;
        seen.extend(step(&mut rt, &mut vm, frame((60.0, 60.0), true)).1);
        seen.extend(step(&mut rt, &mut vm, frame((60.0, 60.0), false)).1);
        assert!(!seen.iter().any(|h| h == "under_click"), "{seen:?}");
        assert_eq!(vm.get("Open"), Some(Value::Bool(true)), "a click inside keeps it open");
        // A click outside it: it closes, and the button there does not get the click.
        let mut seen = step(&mut rt, &mut vm, frame((340.0, 265.0), false)).1;
        seen.extend(step(&mut rt, &mut vm, frame((340.0, 265.0), true)).1);
        seen.extend(step(&mut rt, &mut vm, frame((340.0, 265.0), false)).1);
        assert_eq!(vm.get("Open"), Some(Value::Bool(false)), "light-dismissed");
        assert!(!seen.iter().any(|h| h == "far_click"), "{seen:?}");
        // Closed: the next click reaches the view again.
        let mut seen = step(&mut rt, &mut vm, frame((340.0, 265.0), true)).1;
        seen.extend(step(&mut rt, &mut vm, frame((340.0, 265.0), false)).1);
        assert!(seen.iter().any(|h| h == "far_click"), "{seen:?}");
    }
}
