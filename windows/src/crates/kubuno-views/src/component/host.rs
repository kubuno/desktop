//! [`ControlHost`]: controls built in Rust code, painted and driven outside any `.kbview` view —
//! what WinForms does for controls added with `form.Controls.Add(new RoundButton())`.
//!
//! Each frame the host routes the input to its controls with the same router as a view (EVT-2:
//! MouseEnter/Move/Hover/Leave, MouseDown → Click → MouseClick → MouseUp, keys to the focused
//! control, focus and validation sequences), delivered through each control's `on_…` methods,
//! then paints them (`on_paint_background`, `on_paint`) in the order they were added. It honours
//! the control styles (`SELECTABLE`: focusable by click and Tab; `STANDARD_CLICK`: Click
//! synthesized from a press and release inside; `STANDARD_DOUBLE_CLICK`: DoubleClick on a quick
//! second press, a second Click otherwise; `OPAQUE`: no background pass), `is_input_key(Tab)`
//! (the control keeps Tab), `process_cmd_key` / `process_dialog_key`, `wnd_proc` (the message
//! pre-filter), `focus()`, `invalidate()`, `visible`/`enabled` and `DesignMode`.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;

use kubuno_controls::host::{self, vk, Frame};
use kubuno_controls::ControlCanvas;
use kubuno_ui::{FocusId, FocusOpts, FocusRing, WidgetState};

use super::control::{Control, ControlStyles, Keys};
use super::{Component, Site};
use crate::binding::{HandlerTable, MapViewModel};
use crate::events::router::{Dispatch, FrameInput, InputRouter, SlotEvents};
use crate::events::{Key, MouseButton};
use crate::node::ViewEvent;

/// A control added to a [`ControlHost`]: a shared handle to it (the host keeps one too).
pub struct HostedControl<C: ?Sized> {
    cell: Rc<RefCell<C>>,
    /// Set when the control is lent mutably: the host repaints it (its paint buffer may be stale).
    dirty: Rc<Cell<bool>>,
}

impl<C: ?Sized> Clone for HostedControl<C> {
    fn clone(&self) -> Self {
        Self { cell: self.cell.clone(), dirty: self.dirty.clone() }
    }
}

impl<C: ?Sized> HostedControl<C> {
    /// The control, to read it (between frames).
    pub fn borrow(&self) -> Ref<'_, C> {
        self.cell.borrow()
    }

    /// The control, to change it (between frames: its properties, `focus()`, subscriptions). The host
    /// repaints it at the next frame (what it changed may show).
    pub fn borrow_mut(&self) -> RefMut<'_, C> {
        self.dirty.set(true);
        self.cell.borrow_mut()
    }
}

struct Hosted {
    slot: Rc<SlotEvents>,
    control: Rc<RefCell<dyn Component>>,
    dirty: Rc<Cell<bool>>,
}

/// Hosts controls built in Rust (see the module doc).
///
/// ```no_run
/// use kubuno_views::prelude::*;
/// use kubuno_ui::Rect;
///
/// let mut host = ControlHost::new();
/// let mut ok = Button::new("OK");
/// ok.set_bounds(Rect::new(16.0, 16.0, 116.0, 52.0));
/// let ok = host.add(ok);
/// ok.borrow().click().subscribe(|_, _| tracing::info!("clicked")).detach();
/// // In the window's paint callback: `let events = host.frame(canvas, frame);`
/// ```
pub struct ControlHost {
    focus: FocusRing,
    router: InputRouter,
    controls: Vec<Hosted>,
    vm: MapViewModel,
    handlers: HandlerTable,
    design_mode: bool,
    next: usize,
}

impl Default for ControlHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlHost {
    pub fn new() -> Self {
        Self { focus: FocusRing::new(), router: InputRouter::new(), controls: Vec::new(), vm: MapViewModel::new(), handlers: HandlerTable::new(), design_mode: false, next: 0 }
    }

    /// Adds `control` on top of the others, and returns a handle to it. Its bounds are its own
    /// (`set_bounds`); it is sited under its `name` (a generated one when it has none).
    pub fn add<C: Control>(&mut self, control: C) -> HostedControl<C> {
        let cell = Rc::new(RefCell::new(control));
        let dyn_cell: Rc<RefCell<dyn Component>> = cell.clone();
        let dirty = Rc::new(Cell::new(false));
        self.insert(dyn_cell, dirty.clone());
        HostedControl { cell, dirty }
    }

    /// Adds a control built elsewhere (a class picked at run time: `controls::class_of`).
    pub fn add_dyn(&mut self, control: Rc<RefCell<dyn Component>>) {
        self.insert(control, Rc::new(Cell::new(true)));
    }

    fn insert(&mut self, cell: Rc<RefCell<dyn Component>>, dirty: Rc<Cell<bool>>) {
        let index = self.next;
        self.next += 1;
        let slot = {
            let mut component = cell.borrow_mut();
            let id = format!("host.{index}");
            let mut name = component.display_name().to_string();
            if name.is_empty() {
                name = format!("{}{}", component.class_name().to_ascii_lowercase(), index + 1);
            }
            component.set_site(Some(Site { name: name.clone(), design_mode: self.design_mode, container: None }));
            let mut slot = SlotEvents::new(id, component.class_name());
            slot.name = Some(name.clone());
            slot.report_all = true;
            let keyboard_click = component.as_button_base().is_some();
            if let Some(control) = component.as_control_mut() {
                let focus_id = *control.control_core_mut().focus_id.get_or_insert(FocusId::indexed("kubuno.control-host", index));
                if control.control_core().name.is_empty() {
                    control.control_core_mut().name = name;
                }
                let styles = control.control_core().styles;
                slot.focus_id = Some(focus_id);
                slot.native_click = !styles.contains(ControlStyles::STANDARD_CLICK);
                slot.standard_double_click = styles.contains(ControlStyles::STANDARD_DOUBLE_CLICK);
                slot.keyboard_click = keyboard_click;
            }
            Rc::new(slot)
        };
        self.controls.push(Hosted { slot, control: cell, dirty });
    }

    /// Hosts the controls in design mode (`Component::design_mode`): painted, never routed.
    pub fn set_design_mode(&mut self, on: bool) {
        self.design_mode = on;
        for h in &self.controls {
            if let Ok(mut c) = h.control.try_borrow_mut() {
                let mut site = c.site().cloned().unwrap_or_default();
                site.design_mode = on;
                c.set_site(Some(site));
            }
        }
    }

    /// The focus ring the host's controls take part in.
    pub fn focus_ring(&mut self) -> &mut FocusRing {
        &mut self.focus
    }

    /// Routes the frame's input to the controls, then paints them. Returns every event the
    /// controls raised (after their `on_…` overrides), in order.
    pub fn frame(&mut self, canvas: &dyn ControlCanvas, frame: &Frame) -> Vec<ViewEvent> {
        self.focus.begin_frame(frame);
        let mut events = Vec::new();

        // `Control::focus()` requests, applied before the routing so their focus events follow.
        for h in &self.controls {
            let Ok(mut c) = h.control.try_borrow_mut() else { continue };
            let Some(control) = c.as_control_mut() else { continue };
            if std::mem::take(&mut control.control_core_mut().focus_requested) {
                if let Some(id) = h.slot.focus_id {
                    self.focus.focus_visibly(id);
                }
            }
        }

        let routed = !self.design_mode;
        if routed {
            let now = host::now_ms();
            let input_events = host::events();
            let input = FrameInput { frame, now_ms: now, events: &input_events };
            let outcome = {
                let mut d = Dispatch { vm: &mut self.vm, handlers: &mut self.handlers, events: &mut events };
                self.router.begin_frame(&input, &mut self.focus, &mut d)
            };
            for i in outcome.consumed {
                if let Some(target) = input_events.get(i) {
                    let mut done = false;
                    host::consume(|e| {
                        let hit = !done && e == target;
                        done |= hit;
                        hit
                    });
                }
            }
            if let Some(ms) = outcome.repaint_after {
                host::request_repaint_after(ms);
            }
        }

        let hot = self.router.hot_id().map(str::to_string);
        let captured = self.router.captured().map(|(id, b)| (id.to_string(), b));
        let mut repaint = false;
        for h in &self.controls {
            let Ok(mut c) = h.control.try_borrow_mut() else { continue };
            let design_mode = self.design_mode;
            let Some(control) = c.as_control_mut() else { continue };
            if !control.visible() {
                continue;
            }
            control.create_control();
            let bounds = control.bounds();
            let focus_state = match h.slot.focus_id {
                Some(id) if control.can_select() && !design_mode => {
                    let opts = FocusOpts { wants_tab: control.is_input_key(Keys::plain(Key(vk::TAB))), always_visible: false, skip_tab: !control.control_core().tab_stop };
                    self.focus.register_with(id, bounds, opts)
                }
                _ => Default::default(),
            };
            let is_hot = hot.as_deref() == Some(h.slot.id.as_str());
            let is_pressed = captured.as_ref().is_some_and(|(id, b)| id == &h.slot.id && *b == MouseButton::Left) && is_hot;
            {
                let core = control.control_core_mut();
                core.hot = is_hot;
                core.pressed = is_pressed;
                core.focused = focus_state.focused;
            }
            let state = focus_state.apply(WidgetState::REST.hot(is_hot).pressed(is_pressed)).disabled(!control.enabled());
            // Lent mutably since the last frame: its state may have changed without an invalidate.
            if h.dirty.replace(false) {
                control.invalidate();
            }
            // Background (unless OPAQUE), paint, the paint buffer — super::paint.
            super::paint::paint_control(control, canvas, bounds, state, None, false);
            drop(c);
            if routed {
                self.router.register_with_control(h.slot.clone(), bounds, Some(&h.control));
            }
        }

        if routed {
            let mut d = Dispatch { vm: &mut self.vm, handlers: &mut self.handlers, events: &mut events };
            self.router.end_frame(&mut d);
        }
        self.focus.end_frame();

        // A control invalidated by an event (or `update()`d) asks for the next frame.
        for h in &self.controls {
            if let Ok(c) = h.control.try_borrow() {
                if let Some(control) = c.as_control() {
                    repaint |= control.invalidated_rect().is_some() || control.control_core().update_requested;
                }
            }
        }
        if repaint {
            host::request_repaint_after(0);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{Button, Label, Panel};
    use crate::component::HasControlCore;

    /// What `add` records: the site, the generated name and focus id, and the click behaviour
    /// taken from the control's styles.
    #[test]
    fn adding_sites_the_control_and_reads_its_styles() {
        let mut host = ControlHost::new();
        let mut ok = Button::new("OK");
        ok.set_name("ok");
        let ok = host.add(ok);
        let label = host.add(Label::new("Name"));
        host.add(Panel::new());
        assert_eq!(host.controls.len(), 3);

        let slot = &host.controls[0].slot;
        assert_eq!((slot.element, slot.name.as_deref()), ("Button", Some("ok")));
        assert!(!slot.native_click, "STANDARD_CLICK: the host synthesizes Click");
        assert!(!slot.standard_double_click, "a button's second click is a Click");
        assert!(slot.keyboard_click && slot.report_all);
        assert_eq!(ok.borrow().site().map(|s| s.name.as_str()), Some("ok"));
        assert_eq!(ok.borrow().control_core().focus_id, slot.focus_id);

        let slot = &host.controls[1].slot;
        assert_eq!(slot.name.as_deref(), Some("label2"), "an unnamed control gets its class name and rank");
        assert!(!slot.keyboard_click && slot.standard_double_click);
        assert_eq!(label.borrow().name(), "label2");
        assert!(!label.borrow().can_select());

        host.set_design_mode(true);
        assert!(ok.borrow().design_mode() && label.borrow().design_mode());
    }
}
