//! The named components of a view (`vskubuno/docs/DATA.md` §7, lot DATA-2): the instances the view
//! runtime creates for the non-visual elements of an application or a library (`<BindingSource>`,
//! `<ErrorProvider>`… from `kubuno-desktop-data`), reachable by `x:Name`, and what makes them take part in
//! data binding without any code in the view model.
//!
//! - [`ComponentScope`]: the view's components by name. The runtime owns it (a hot reload keeps an
//!   instance whose element keeps its name and class); code reaches it with
//!   [`crate::runtime::Runtime::components`], [`current`] (inside a handler, a timer tick, a task) or
//!   [`crate::events::UiHandle::components`] (an async handler), then
//!   [`ComponentScope::with::<BindingSource, _>("customers", |bs| …)`](ComponentScope::with).
//! - [`BindingProvider`]: a component that answers binding paths below its name
//!   (`{Binding Source=customers, Path=Name}` is the path `customers.Name`). The runtime paints the
//!   view with a [`ScopedViewModel`] around the application's view model: a path whose first
//!   segment names a provider goes to it, every other path to the view model — so a view model no
//!   longer forwards anything.
//! - **Events.** A component raises its events to its Rust subscribers at once. For the `.kbview`
//!   handler its element names (`OnRowValidating="…"`), [`raise_now`] runs the handler
//!   **synchronously** when the component was reached through a binding (a control's write, the
//!   runtime's per-frame sync), so a cancelable event (`RowValidating`, `AddingNew`) can be cancelled
//!   by its XML handler. Reached from code, the event is queued on the component
//!   (`ComponentCore::queue_event`) and delivered by the runtime in the same frame (the view model is
//!   borrowed by the code that made the change).
//! - **Error glyphs.** [`BindingProvider::field_error`] tells the runtime which bound fields are in
//!   error: it draws the ErrorProvider glyph next to every control bound to one (WinForms'
//!   adornment), with the message as its tooltip.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use crate::binding::{BindingFormat, BindingSpec, HandlerTable, Value, ViewModel};
use crate::component::Component;
use crate::events::router::SlotEvents;
use crate::events::{ElementRef, EventArgs};
use crate::format::ValueKind;
use crate::node::{ViewEvent, ViewEventKind};
use kubuno_desktop_ui::Rect;

/// A component that answers the binding paths below its name (see the module doc).
pub trait BindingProvider {
    /// What a binding reads at `path` (below the component's name, `""` for the component itself)
    /// for a property of shape `want`, formatted per `format`. `None`: nothing there (the property
    /// falls back to its default).
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, scope: &ComponentScope) -> Option<Value>;

    /// What a two-way binding writes at `path`, parsed per `format`. Returns whether the path
    /// belongs to the component (a refused value is still "handled": the component reports it).
    fn binding_set(&mut self, path: &str, value: Value, format: &BindingFormat, scope: &ComponentScope) -> bool;

    /// Called by the runtime once per frame before the paint, and after every binding write:
    /// follows the other components it depends on (a detail list follows its master's current row).
    /// Returns whether it changed.
    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        false
    }

    /// The error to show next to a control bound to `path` (a full path, `customers.Email`), if
    /// this component reports one (an `ErrorProvider`).
    fn field_error(&self, _path: &str, _scope: &ComponentScope) -> Option<FieldError> {
        None
    }
}

/// Where the error glyph sits next to its control (WinForms `ErrorIconAlignment`).
#[derive(crate::component::PropertyValue, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorIconAlignment {
    TopLeft,
    TopRight,
    MiddleLeft,
    #[default]
    MiddleRight,
    BottomLeft,
    BottomRight,
}

/// When the error glyph blinks (WinForms `ErrorBlinkStyle`).
#[derive(crate::component::PropertyValue, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorBlinkStyle {
    /// Blinks a few times when it appears, and again when the message changes.
    #[default]
    BlinkIfDifferentError,
    /// Blinks as long as it is shown.
    AlwaysBlink,
    /// Never blinks.
    NeverBlink,
}

/// An error a control shows ([`BindingProvider::field_error`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldError {
    pub message: String,
    pub alignment: ErrorIconAlignment,
    /// The gap between the control and the glyph, in DIP.
    pub padding: f32,
    pub blink: ErrorBlinkStyle,
    /// The blink half-period, in milliseconds.
    pub blink_rate: u32,
}

/// Named instances kept across a hot reload: `(name, class, instance)`.
pub(crate) type Reusable = Vec<(String, &'static str, Rc<RefCell<dyn Component>>)>;

/// A component and its element.
type Target = (Rc<RefCell<dyn Component>>, Rc<SlotEvents>);

/// One named component of a view.
pub(crate) struct Entry {
    pub name: String,
    pub class: &'static str,
    /// The element's slot owns the instance; a view closed or reloaded without it drops it.
    pub instance: Weak<RefCell<dyn Component>>,
    /// The element as the router sees it: its `On*` handlers.
    pub slot: Rc<SlotEvents>,
}

/// The named components of a view (see the module doc). Cloning shares it.
#[derive(Clone, Default)]
pub struct ComponentScope {
    entries: Rc<RefCell<Vec<Entry>>>,
}

impl std::fmt::Debug for ComponentScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.entries.borrow().iter().map(|e| format!("{}: {}", e.name, e.class))).finish()
    }
}

impl ComponentScope {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the components (a view compiled or reloaded).
    pub(crate) fn replace(&self, entries: Vec<Entry>) {
        *self.entries.borrow_mut() = entries;
    }

    /// Adds a component outside any view (tests, tools, `kubuno_desktop_data::DataContext`): the scope holds it
    /// weakly, the caller keeps it alive. `slot`: its element (the `On*` handlers), else none.
    pub fn insert(&self, name: &str, instance: Rc<RefCell<dyn Component>>, slot: Option<Rc<SlotEvents>>) -> Rc<RefCell<dyn Component>> {
        let class = instance.borrow().class_name();
        if let Ok(mut c) = instance.try_borrow_mut() {
            c.set_site(Some(crate::component::Site { name: name.to_string(), design_mode: false, container: None }));
        }
        let slot = slot.unwrap_or_else(|| {
            let mut s = SlotEvents::new(String::new(), class);
            s.name = Some(name.to_string());
            Rc::new(s)
        });
        self.entries.borrow_mut().push(Entry { name: name.to_string(), class, instance: Rc::downgrade(&instance), slot });
        instance
    }

    /// The live instances with their names and classes (what a hot reload keeps).
    pub(crate) fn reusable(&self) -> Reusable {
        self.entries.borrow().iter().filter_map(|e| e.instance.upgrade().map(|i| (e.name.clone(), e.class, i))).collect()
    }

    /// The names of the components, in view order.
    pub fn names(&self) -> Vec<String> {
        self.entries.borrow().iter().map(|e| e.name.clone()).collect()
    }

    /// The component named `name`.
    pub fn get(&self, name: &str) -> Option<Rc<RefCell<dyn Component>>> {
        self.entries.borrow().iter().find(|e| e.name == name).and_then(|e| e.instance.upgrade())
    }

    /// The class of the component named `name` (`"BindingSource"`).
    pub fn class_of(&self, name: &str) -> Option<&'static str> {
        self.entries.borrow().iter().find(|e| e.name == name).map(|e| e.class)
    }

    /// The element of the component named `name`.
    pub(crate) fn slot(&self, name: &str) -> Option<Rc<SlotEvents>> {
        self.entries.borrow().iter().find(|e| e.name == name).map(|e| e.slot.clone())
    }

    /// Runs `f` on the component named `name`, as a `T` (its class or a base class). `None` when
    /// there is none, it is not a `T`, or it is busy (borrowed by the code that called this one).
    /// Its events reach the view's handlers at the end of the current step of the frame.
    pub fn with<T: Component, R>(&self, name: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let cell = self.get(name)?;
        let Ok(mut c) = cell.try_borrow_mut() else {
            tracing::warn!(component = name, "the component is busy (a handler of its own event reached it again): not run");
            return None;
        };
        let t = c.find_base_mut::<T>()?;
        let out = f(t);
        drop(c);
        // What depends on the component follows it now (a detail list, its master's current row).
        self.sync_all(&mut crate::binding::MapViewModel::new(), false);
        Some(out)
    }

    /// [`Self::with`], read-only.
    pub fn with_ref<T: Component, R>(&self, name: &str, f: impl FnOnce(&T) -> R) -> Option<R> {
        let cell = self.get(name)?;
        let c = cell.try_borrow().ok()?;
        let t = c.find_base::<T>()?;
        Some(f(t))
    }

    /// [`Self::with`], with the `.kbview` handlers of the events `f` raises run synchronously
    /// against `vm` (a cancelable event can then be cancelled by its XML handler). `vm` is the view
    /// model the view is painted with (in an async handler: inside `ui.update(|vm| …)`).
    pub fn with_dispatch<T: Component, R>(&self, vm: &mut dyn ViewModel, name: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let scope = self.clone();
        let mut sink = |component: &str, event: &'static str, args: &mut dyn EventArgs| dispatch_to(&scope, &mut *vm, component, event, args);
        let out = with_sink(&mut sink, || self.with(name, f));
        self.sync_all(&mut *vm, true);
        out
    }

    /// `vm` seen through the scope: the paths that name a component go to it (what the runtime
    /// paints with). `live`: run the XML handlers of the events it raises synchronously.
    pub fn view_model<'a>(&'a self, vm: &'a mut dyn ViewModel, live: bool) -> ScopedViewModel<'a> {
        ScopedViewModel { inner: vm, scope: self, live }
    }

    /// The component a path names, and the rest of the path.
    fn provider_of<'p>(&self, path: &'p str) -> Option<(Rc<RefCell<dyn Component>>, &'p str)> {
        let (name, rest) = path.split_once('.').unwrap_or((path, ""));
        let cell = self.get(name)?;
        let is_provider = cell.try_borrow().map(|c| c.as_binding_provider().is_some()).unwrap_or(false);
        is_provider.then_some((cell, rest))
    }

    /// Lets every provider follow the others ([`BindingProvider::binding_sync`]); `live`: with the
    /// XML handlers run synchronously against `vm`. Returns whether anything changed.
    pub fn sync_all(&self, vm: &mut dyn ViewModel, live: bool) -> bool {
        let cells: Vec<Rc<RefCell<dyn Component>>> = self.entries.borrow().iter().filter_map(|e| e.instance.upgrade()).collect();
        let mut changed = false;
        for cell in cells {
            let Ok(mut c) = cell.try_borrow_mut() else { continue };
            let Some(p) = c.as_binding_provider_mut() else { continue };
            changed |= if live {
                let scope = self.clone();
                let vm = &mut *vm;
                let mut sink = |component: &str, event: &'static str, args: &mut dyn EventArgs| dispatch_to(&scope, &mut *vm, component, event, args);
                with_sink(&mut sink, || p.binding_sync(self))
            } else {
                p.binding_sync(self)
            };
        }
        changed
    }

    /// Delivers the events the components queued (raised from code) to their elements' handlers,
    /// reports them in `events`. Returns how many ran.
    pub(crate) fn deliver(&self, handlers: &mut HandlerTable, vm: &mut dyn ViewModel, events: &mut Vec<ViewEvent>) -> usize {
        let targets: Vec<Target> = self.entries.borrow().iter().filter_map(|e| e.instance.upgrade().map(|i| (i, e.slot.clone()))).collect();
        let mut ran = 0;
        for (cell, slot) in targets {
            // Taken out first: a handler may use the component.
            let queued = match cell.try_borrow_mut() {
                Ok(mut c) if c.component_core().has_queued() => c.component_core_mut().take_queued(),
                _ => continue,
            };
            for (event, mut args) in queued {
                let Some(handler) = slot.handler(event) else { continue };
                let sender = slot.sender(Rect::default());
                handlers.dispatch_args(handler, &mut *vm, &sender, args.as_mut());
                ran += 1;
                let args: Rc<dyn EventArgs> = Rc::from(args);
                events.push(ViewEvent { focus_id: slot.focus_id, handler: Some(handler.to_string()), kind: ViewEventKind::Other { name: event.strip_prefix("On").unwrap_or(event), args } });
            }
        }
        ran
    }

    /// The error of the field a control is bound to at `path`, if a component reports one.
    pub fn field_error(&self, path: &str) -> Option<FieldError> {
        let cells: Vec<Rc<RefCell<dyn Component>>> = self.entries.borrow().iter().filter_map(|e| e.instance.upgrade()).collect();
        cells.iter().find_map(|cell| cell.try_borrow().ok().and_then(|c| c.as_binding_provider().and_then(|p| p.field_error(path, self))))
    }

    /// Whether any component answers binding paths.
    pub fn has_providers(&self) -> bool {
        self.entries.borrow().iter().filter_map(|e| e.instance.upgrade()).any(|c| c.try_borrow().map(|c| c.as_binding_provider().is_some()).unwrap_or(false))
    }
}

/// The components of the view whose frame is running on this thread (inside a handler, a timer
/// tick, an async task, a posted closure). `None` outside a view's frame.
pub fn current() -> Option<ComponentScope> {
    crate::events::executor::current_scope()
}

/// Runs the `.kbview` handler of `event` for the component `component` of `scope` (the sink the
/// scope installs): `true` when it ran, or when the element names none (nothing to queue).
fn dispatch_to(scope: &ComponentScope, vm: &mut dyn ViewModel, component: &str, event: &'static str, args: &mut dyn EventArgs) -> bool {
    let Some(slot) = scope.slot(component) else { return false };
    let Some(handler) = slot.handler(event) else { return true };
    let sender = slot.sender(Rect::default());
    vm.dispatch_event(handler, &sender, args)
}

// ── The synchronous event sink ───────────────────────────────────────────────────────────────

type SinkFn<'a> = dyn FnMut(&str, &'static str, &mut dyn EventArgs) -> bool + 'a;

thread_local! {
    /// The sink lent by [`with_sink`] while a component is called through a binding: taken out
    /// (set to `None`) while it runs, so a handler reaching another component queues instead of
    /// making a second `&mut` of the view model.
    static SINK: Cell<Option<NonNull<SinkFn<'static>>>> = const { Cell::new(None) };
}

/// Runs `f` with `sink` as the synchronous event sink (see [`raise_now`]).
fn with_sink<R>(sink: &mut SinkFn<'_>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<NonNull<SinkFn<'static>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SINK.with(|s| s.set(self.0));
        }
    }
    let ptr: NonNull<SinkFn<'_>> = NonNull::from(sink);
    // SAFETY: only the lifetime is erased (same fat-pointer layout). The pointer lives in `SINK`
    // for the duration of this call only (restored by `Restore`, even on a panic), during which
    // `sink` stays exclusively borrowed by this function; `raise_now` dereferences it only while it
    // is taken out of `SINK`, so two `&mut` never coexist.
    let ptr: NonNull<SinkFn<'static>> = unsafe { std::mem::transmute::<NonNull<SinkFn<'_>>, NonNull<SinkFn<'static>>>(ptr) };
    let _restore = Restore(SINK.with(|s| s.replace(Some(ptr))));
    f()
}

/// Whether an event raised now would reach its `.kbview` handler synchronously ([`raise_now`] would
/// run it): the component is being called through a binding or the runtime, not from a handler that
/// holds the view model. A component that must run the handlers of the events it raises (a print
/// job's `PrintPage`) defers its work to [`BindingProvider::binding_sync`] when this is `false`.
pub fn can_raise_now() -> bool {
    SINK.with(|s| s.get().is_some())
}

/// Runs the `.kbview` handler of `event` raised by the component named `component` now, when the
/// component is being called through a binding or the runtime (see the module doc). Returns
/// whether it did — `false`: queue the event for the element's handler instead.
pub fn raise_now(component: &str, event: &'static str, args: &mut dyn EventArgs) -> bool {
    let Some(ptr) = SINK.with(|s| s.take()) else { return false };
    struct Restore(NonNull<SinkFn<'static>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SINK.with(|s| s.set(Some(self.0)));
        }
    }
    let restore = Restore(ptr);
    // SAFETY: see `with_sink`: the pointer is valid while it can be found in `SINK`, and it was
    // taken out, so this is the only reference to the sink until `restore` puts it back.
    let sink = unsafe { &mut *restore.0.as_ptr() };
    sink(component, event, args)
}

// ── ScopedViewModel ──────────────────────────────────────────────────────────────────────────

/// A view model seen through a [`ComponentScope`] (see the module doc).
pub struct ScopedViewModel<'a> {
    inner: &'a mut dyn ViewModel,
    scope: &'a ComponentScope,
    live: bool,
}

impl<'a> ScopedViewModel<'a> {
    pub fn new(inner: &'a mut dyn ViewModel, scope: &'a ComponentScope, live: bool) -> Self {
        Self { inner, scope, live }
    }
}

impl ViewModel for ScopedViewModel<'_> {
    fn get(&self, path: &str) -> Option<Value> {
        self.get_bound(&BindingSpec::of(path), ValueKind::Any)
    }

    fn set(&mut self, path: &str, value: Value) {
        self.set_bound(&BindingSpec::of(path), value);
    }

    fn get_bound(&self, spec: &BindingSpec, want: ValueKind) -> Option<Value> {
        match self.scope.provider_of(&spec.path) {
            Some((cell, rest)) => {
                let c = cell.try_borrow().ok()?;
                c.as_binding_provider()?.binding_get(rest, want, &spec.format, self.scope)
            }
            None => self.inner.get_bound(spec, want),
        }
    }

    fn set_bound(&mut self, spec: &BindingSpec, value: Value) {
        let Some((cell, rest)) = self.scope.provider_of(&spec.path) else {
            self.inner.set_bound(spec, value);
            return;
        };
        {
            let Ok(mut c) = cell.try_borrow_mut() else {
                tracing::warn!(path = %spec.path, "the component is busy: the binding write is dropped");
                return;
            };
            let Some(p) = c.as_binding_provider_mut() else { return };
            if self.live {
                let scope = self.scope.clone();
                let vm = &mut *self.inner;
                let mut sink = |component: &str, event: &'static str, args: &mut dyn EventArgs| dispatch_to(&scope, &mut *vm, component, event, args);
                with_sink(&mut sink, || p.binding_set(rest, value, &spec.format, self.scope));
            } else {
                p.binding_set(rest, value, &spec.format, self.scope);
            }
        }
        // What depends on the component follows it (a detail list, its master's current row).
        self.scope.sync_all(&mut *self.inner, self.live);
    }

    fn dispatch_event(&mut self, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        self.inner.dispatch_event(handler, sender, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::MapViewModel;
    use crate::component::{ComponentCore, PropertyValue};
    use crate::events::CancelEventArgs;

    /// A provider holding one number, `counter.Value`, raising a cancelable `Changing` first.
    #[derive(crate::component::Component, Default)]
    #[kubuno(extends = Component, overrides(Component))]
    struct Counter {
        base: ComponentCore,
        value: f32,
    }

    impl Component for Counter {
        fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
            Some(self)
        }
        fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
            Some(self)
        }
    }

    impl BindingProvider for Counter {
        fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
            match path {
                "Value" => crate::format::to_target(Value::F32(self.value), want, format),
                _ => None,
            }
        }
        fn binding_set(&mut self, path: &str, value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
            if path != "Value" {
                return false;
            }
            let mut args = CancelEventArgs::default();
            if !raise_now("counter", "OnChanging", &mut args) {
                self.base.queue_event("OnChanging", Box::new(args));
            }
            if !args.cancel {
                self.value = f32::from_value(&value).unwrap_or(self.value);
            }
            true
        }
    }

    struct Vm {
        inner: MapViewModel,
        refuse: bool,
        calls: u32,
    }

    impl ViewModel for Vm {
        fn get(&self, path: &str) -> Option<Value> {
            self.inner.get(path)
        }
        fn set(&mut self, path: &str, value: Value) {
            self.inner.set(path, value)
        }
        fn dispatch_event(&mut self, handler: &str, _sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
            if handler == "counter_changing" {
                self.calls += 1;
                if let Some(a) = args.downcast_mut::<CancelEventArgs>() {
                    a.cancel = self.refuse;
                }
                return true;
            }
            false
        }
    }

    fn scope() -> (ComponentScope, Rc<RefCell<dyn Component>>) {
        let scope = ComponentScope::new();
        let mut slot = SlotEvents::new("0.1", "Counter");
        slot.name = Some("counter".into());
        let slot = slot.with_handler("OnChanging", "counter_changing");
        let c: Rc<RefCell<dyn Component>> = Rc::new(RefCell::new(Counter::default()));
        scope.insert("counter", c.clone(), Some(Rc::new(slot)));
        (scope, c)
    }

    #[test]
    fn paths_that_name_a_component_go_to_it_and_xml_handlers_can_cancel() {
        let (scope, _c) = scope();
        let mut vm = Vm { inner: MapViewModel::new().with("Title", Value::Str("t".into())), refuse: false, calls: 0 };
        {
            let mut scoped = scope.view_model(&mut vm, true);
            assert_eq!(scoped.get("Title"), Some(Value::Str("t".into())), "other paths reach the view model");
            scoped.set("counter.Value", Value::F32(3.0));
            let spec = crate::binding::parse_binding("{Binding Source=counter, Path=Value, FormatString=N1, Culture=en-US}").expect("binding");
            assert_eq!(scoped.get_bound(&spec, ValueKind::Text), Some(Value::Str("3.0".into())));
        }
        assert_eq!(vm.calls, 1, "the handler ran synchronously");
        vm.refuse = true;
        scope.view_model(&mut vm, true).set("counter.Value", Value::F32(9.0));
        assert_eq!(scope.with_ref::<Counter, _>("counter", |c| c.value), Some(3.0), "the XML handler cancelled the change");
        assert!(scope.with_ref::<Counter, _>("counter", |c| !c.base.has_queued()).unwrap_or(false), "nothing was queued");
    }

    #[test]
    fn events_raised_from_code_are_queued_then_delivered() {
        let (scope, _c) = scope();
        let mut vm = Vm { inner: MapViewModel::new(), refuse: false, calls: 0 };
        // Outside any binding: no sink, the event waits on the component.
        scope.with::<Counter, _>("counter", |c| c.binding_set("Value", Value::F32(1.0), &BindingFormat::default(), &ComponentScope::new()));
        assert_eq!(vm.calls, 0);
        let mut events = Vec::new();
        let mut table = HandlerTable::new();
        assert_eq!(scope.deliver(&mut table, &mut vm, &mut events), 1);
        assert_eq!(vm.calls, 1);
        assert_eq!(events.len(), 1);
        // A component busy in a handler is reported, not a panic.
        let cell = scope.get("counter").expect("counter");
        let _guard = cell.borrow_mut();
        assert!(scope.with::<Counter, _>("counter", |_| ()).is_none());
        assert_eq!(scope.class_of("counter"), Some("Counter"));
    }

    #[test]
    fn raise_now_without_a_sink_does_nothing() {
        assert!(!raise_now("x", "OnChanging", &mut CancelEventArgs::default()));
    }
}
