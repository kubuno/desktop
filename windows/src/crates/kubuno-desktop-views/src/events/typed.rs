//! Typed handlers (`vskubuno/docs/EVENTS.md` §5.4, work package EVT-4).
//!
//! The code-behind of a view declares its handlers as ordinary methods of its view model,
//! in an `impl` marked `#[kubuno_desktop_views::event_handlers]`:
//!
//! ```
//! use kubuno_desktop_views::prelude::*;
//! use kubuno_desktop_views::events::dispatch_typed;
//!
//! #[derive(Default)]
//! struct MainViewModel { status: String, clicks: u32 }
//!
//! impl ViewModel for MainViewModel {
//!     fn get(&self, path: &str) -> Option<Value> {
//!         (path == "Status").then(|| Value::Str(self.status.clone()))
//!     }
//!     fn set(&mut self, _path: &str, _value: Value) {}
//! }
//!
//! #[kubuno_desktop_views::event_handlers]
//! impl MainViewModel {
//!     fn say_hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {
//!         self.clicks += 1;
//!         self.status = format!("{} clicked at {}, {}", sender.text(), e.x, e.y);
//!     }
//!
//!     fn name_key_down(&mut self, e: &mut KeyEventArgs) {
//!         e.handled = true; // the text field never sees the key
//!     }
//! }
//!
//! let attrs = vec![("Text".to_string(), "Hello".to_string())];
//! let sender = ElementRef { name: None, element: "Button", id: "0", bounds: Default::default(),
//!                           focus_id: None, attributes: &attrs };
//! let mut vm = MainViewModel::default();
//! let mut e = MouseEventArgs { x: 4.0, y: 2.0, ..Default::default() };
//! assert!(dispatch_typed(&mut vm, "say_hello_click", &sender, &mut e));
//! assert_eq!(vm.status, "Hello clicked at 4, 2");
//! assert_eq!(MainViewModel::HANDLERS[0].sender, Some("Button"));
//! ```
//!
//! The attribute macro implements [`EventSink`] for the type: a `match` from handler name to
//! method, a checked downcast of the event's `&mut dyn EventArgs` to the declared args type
//! ([`with_args`]), a checked typed sender ([`HandlerContext::typed_sender`]), and a
//! [`EventSink::HANDLERS`] table. [`crate::runtime::Runtime::frame_typed`] then dispatches the
//! view's events to the sink: the handlers get `&mut self` of the concrete view model.
//!
//! **Accepted signatures** (a receiver `&mut self` or `&self`, then):
//!
//! | Parameters | Meaning |
//! |---|---|
//! | none | runs for any event bound to it |
//! | `e: &A` / `e: &mut A` | `A` an args type (`MouseEventArgs`…); `&mut` to set `handled`/`cancel` |
//! | `e: &dyn EventArgs` / `&mut dyn EventArgs` | any event's args (WinForms' `EventArgs e`) |
//! | `sender: &Sender<C>` | a typed sender (`C` from [`crate::controls`], or [`AnyElement`]) |
//! | `sender: &ElementRef` | an untyped sender |
//! | `sender, e` | both, in that order |
//!
//! A handler declared with a base args type receives a compatible event too: `&EmptyEventArgs`
//! any event, `&mut CancelEventArgs` any cancelable event (its `cancel` is written back),
//! `&mut HandledEventArgs` any event with `handled`. An event whose args or sender do not fit
//! the declaration (an `OnKeyDown` bound to a `&MouseEventArgs` handler, a `Sender<Button>`
//! handler bound to a `<Switch>`) does not call the handler and logs a `tracing::warn!`.
//! Associated functions without `self` are not handlers; a method is excluded with
//! `#[handler(skip)]` and bound under another name with `#[handler(name = "other_name")]`.
//!
//! **Legacy tables** keep working: `HandlerTable`/`handlers!` are untouched, and
//! [`crate::runtime::Runtime::frame_typed_with`] takes one next to the sink (the sink is tried
//! first, then the table's typed entries, then its legacy entries with
//! [`EventArgs::legacy_value`], exactly the value that event always produced).

use std::any::{Any, TypeId};

use crate::binding::{Value, ViewModel};

use super::{ArgsChain, CancelEventArgs, ElementProps, ElementType, ElementRef, EmptyEventArgs, EventArgs, HandledEventArgs, ReadOnlyArgs, Sender};

/// One handler of an [`EventSink`], as `#[event_handlers]` declares it (diagnostics, tooling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandlerInfo {
    /// The name `On*="…"` attributes use (the method's name unless `#[handler(name = …)]`).
    pub name: &'static str,
    /// The method implementing it.
    pub method: &'static str,
    /// The element the sender is typed with (`"Button"`), `"*"` for `Sender<AnyElement>` or
    /// `&ElementRef`, `None` when the handler takes no sender.
    pub sender: Option<&'static str>,
    /// The declared args type's tooling name (`"MouseEventArgs"`, `"EventArgs"` for the root
    /// or `dyn EventArgs`), `None` when the handler takes no args.
    pub args: Option<&'static str>,
    /// The args are taken as `&mut`.
    pub args_mut: bool,
    /// An `async fn` handler (EVT-6): it runs as a task on the UI thread, with a
    /// [`super::UiHandle`] and a copy of the args (never `&mut`).
    pub asynchronous: bool,
}

impl HandlerInfo {
    /// A handler named `name`, implemented by `method`, taking neither sender nor args. The
    /// generated code builds every `HandlerInfo` through these `const` builders, never with a
    /// struct literal, so a field added here later does not break code expanded by an older
    /// `#[event_handlers]` (a stale proc-macro build, rust-analyzer's cached expansion).
    pub const fn new(name: &'static str, method: &'static str) -> Self {
        Self { name, method, sender: None, args: None, args_mut: false, asynchronous: false }
    }

    /// The element the sender is typed with (`"*"` for any).
    pub const fn with_sender(mut self, element: &'static str) -> Self {
        self.sender = Some(element);
        self
    }

    /// The declared args type's tooling name, and whether it is taken as `&mut`.
    pub const fn with_args(mut self, args: &'static str, args_mut: bool) -> Self {
        self.args = Some(args);
        self.args_mut = args_mut;
        self
    }

    /// An `async fn` handler.
    pub const fn asynchronous(mut self) -> Self {
        self.asynchronous = true;
        self
    }
}

/// What a typed handler's generated dispatch code receives besides the args: the sender, and
/// the sender's resolved properties when the handler asked for a typed sender.
pub struct HandlerContext<'a> {
    sender: ElementRef<'a>,
    props: Option<&'a ElementProps>,
}

static NO_PROPS: ElementProps = ElementProps::empty();

impl<'a> HandlerContext<'a> {
    pub fn new(sender: ElementRef<'a>, props: Option<&'a ElementProps>) -> Self {
        Self { sender, props }
    }

    /// The untyped sender.
    pub fn element(&self) -> &ElementRef<'a> {
        &self.sender
    }

    /// The sender typed as `C` — `None` (and a warning naming `handler`) when the element is
    /// not a `C` (`C::ELEMENT` is `"*"` for any element).
    pub fn typed_sender<C: ElementType<Resolved = ElementProps>>(&self, handler: &str) -> Option<Sender<'_, C>> {
        if C::ELEMENT != "*" && C::ELEMENT != self.sender.element {
            tracing::warn!(
                handler,
                "handler `{handler}` takes a `Sender<{}>` but the event was raised by a <{}> ({}): not called",
                C::ELEMENT,
                self.sender.element,
                self.sender.display_name()
            );
            return None;
        }
        Some(Sender::new(self.sender, self.props.unwrap_or(&NO_PROPS)))
    }
}

/// A view model whose handlers are typed methods (`#[kubuno_desktop_views::event_handlers]`
/// implements it; see the module documentation).
pub trait EventSink {
    /// Every handler, in declaration order.
    const HANDLERS: &'static [HandlerInfo];

    /// Runs the handler named `handler` with `args`. Returns whether the sink has a handler of
    /// that name — also when its args or sender did not fit and it was skipped (with a
    /// warning), so a same-named legacy handler never runs in its place.
    fn handle_event(&mut self, handler: &str, cx: &HandlerContext<'_>, args: &mut dyn EventArgs) -> bool;

    /// The handler named `name`, if the sink has one.
    fn handler_info(name: &str) -> Option<&'static HandlerInfo>
    where
        Self: Sized,
    {
        Self::HANDLERS.iter().find(|h| h.name == name)
    }
}

/// Dispatches one event to a typed sink: resolves the sender's properties when the handler
/// takes a typed sender, then calls it. Returns whether `vm` has a handler named `handler`.
pub fn dispatch_typed<V: ViewModel + EventSink>(vm: &mut V, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
    let Some(info) = V::handler_info(handler) else { return false };
    let props = info.sender.map(|_| ElementProps::resolve(sender.name, sender.attributes, &*vm));
    let cx = HandlerContext::new(*sender, props.as_ref());
    vm.handle_event(handler, &cx, args)
}

/// Calls `f` with the event's args as the declared type `T`: the args themselves when they
/// are a `T`; a fresh [`EmptyEventArgs`] when `T` is the root; for `T` =
/// [`CancelEventArgs`] / [`HandledEventArgs`] a bridge over any cancelable / handled args,
/// whose flag is written back. Otherwise `f` is not called: `None` and a warning naming
/// `handler`.
pub fn with_args<T: EventArgs + ArgsChain, R>(args: &mut dyn EventArgs, handler: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    if let Some(exact) = args.downcast_mut::<T>() {
        return Some(f(exact));
    }
    let wanted = TypeId::of::<T>();
    if wanted == TypeId::of::<EmptyEventArgs>() {
        let mut base = EmptyEventArgs;
        return (&mut base as &mut dyn Any).downcast_mut::<T>().map(f);
    }
    if wanted == TypeId::of::<CancelEventArgs>() {
        if let Some(cancel) = args.as_cancelable().map(|c| c.cancel()) {
            let mut bridge = CancelEventArgs { cancel };
            let result = (&mut bridge as &mut dyn Any).downcast_mut::<T>().map(f);
            if let Some(c) = args.as_cancelable_mut() {
                c.set_cancel(bridge.cancel);
            }
            return result;
        }
    }
    if wanted == TypeId::of::<HandledEventArgs>() {
        if let Some(handled) = args.as_handled().map(|h| h.handled()) {
            let mut bridge = HandledEventArgs { handled };
            let result = (&mut bridge as &mut dyn Any).downcast_mut::<T>().map(f);
            if let Some(h) = args.as_handled_mut() {
                h.set_handled(bridge.handled);
            }
            return result;
        }
    }
    tracing::warn!(
        handler,
        "handler `{handler}` takes `{}` but the event carries `{}`: not called",
        T::RUST_TYPE,
        args.type_chain().first().copied().unwrap_or("EventArgs")
    );
    None
}

/// The copy of the args an `async fn` handler receives (EVT-6). Only for [`ReadOnlyArgs`]:
/// `handled`/`cancel` set in a copy, after the event is over, would be silently ignored.
pub fn copy_for_async<T: ReadOnlyArgs + Clone>(args: &mut T) -> T {
    args.clone()
}

/// A view model seen through its typed sink: forwards `get`/`set`, and answers
/// [`ViewModel::dispatch_event`] with [`dispatch_typed`] (then the view model's own
/// `dispatch_event`). What [`crate::runtime::Runtime::frame_typed`] paints with.
pub struct TypedViewModel<'a, V>(pub &'a mut V);

impl<V: ViewModel + EventSink> ViewModel for TypedViewModel<'_, V> {
    fn get(&self, path: &str) -> Option<Value> {
        self.0.get(path)
    }

    fn set(&mut self, path: &str, value: Value) {
        self.0.set(path, value)
    }

    fn get_bound(&self, spec: &crate::binding::BindingSpec, want: crate::format::ValueKind) -> Option<Value> {
        self.0.get_bound(spec, want)
    }

    fn set_bound(&mut self, spec: &crate::binding::BindingSpec, value: Value) {
        self.0.set_bound(spec, value)
    }

    fn dispatch_event(&mut self, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        dispatch_typed(self.0, handler, sender, args) || self.0.dispatch_event(handler, sender, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::MapViewModel;
    use crate::controls::{Button, Switch};
    use crate::events::AnyElement;
    use crate::events::{CheckedChangedEventArgs, ChangeSource, FormClosingEventArgs, Key, KeyEventArgs, MouseButton, MouseEventArgs};
    use crate::handlers;
    use kubuno_desktop_controls::host::Modifiers;

    #[derive(Default)]
    struct Vm {
        log: Vec<String>,
        status: String,
    }

    impl ViewModel for Vm {
        fn get(&self, path: &str) -> Option<Value> {
            match path {
                "Status" => Some(Value::Str(self.status.clone())),
                _ => None,
            }
        }
        fn set(&mut self, path: &str, value: Value) {
            if let ("Status", Value::Str(s)) = (path, value) {
                self.status = s;
            }
        }
    }

    #[crate::event_handlers]
    impl Vm {
        fn ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {
            self.log.push(format!("click {} {:?} {} {},{}", sender.text(), e.button, e.clicks, e.x, e.y));
        }

        fn no_params(&mut self) {
            self.log.push("no params".into());
        }

        fn args_only(&mut self, e: &mut KeyEventArgs) {
            self.log.push(format!("key {:?}", e.key));
            e.handled = true;
        }

        fn any_args(&self, e: &dyn EventArgs) {
            let _ = e;
        }

        fn any_args_mut(&mut self, e: &mut dyn EventArgs) {
            self.log.push(format!("any {}", e.type_chain()[0]));
        }

        fn untyped_sender(&mut self, sender: &ElementRef, e: &EmptyEventArgs) {
            let _ = e;
            self.log.push(format!("from {}", sender.display_name()));
        }

        fn any_element(&mut self, sender: &Sender<AnyElement>) {
            self.log.push(format!("any element {}", sender.element().element));
        }

        fn validating(&mut self, e: &mut CancelEventArgs) {
            e.cancel = true;
        }

        fn toggled(&mut self, sender: &Sender<Switch>, e: &CheckedChangedEventArgs) {
            self.log.push(format!("toggled {} -> {} ({})", sender.string("IsOn").unwrap_or_default(), e.new, sender.name().unwrap_or("")));
        }

        #[handler(name = "renamed")]
        fn renamed_method(&mut self) {
            self.log.push("renamed".into());
        }

        #[handler(skip)]
        fn helper(&mut self, n: u32) -> u32 {
            n + 1
        }

        fn new_vm() -> Self {
            Self::default()
        }
    }

    fn sender<'a>(element: &'static str, name: Option<&'a str>, attributes: &'a [(String, String)]) -> ElementRef<'a> {
        ElementRef { name, element, id: "0", bounds: Default::default(), focus_id: None, attributes }
    }

    #[test]
    fn handlers_table_lists_every_handler_with_its_shape() {
        let names: Vec<_> = Vm::HANDLERS.iter().map(|h| h.name).collect();
        assert_eq!(names, ["ok_click", "no_params", "args_only", "any_args", "any_args_mut", "untyped_sender", "any_element", "validating", "toggled", "renamed"]);
        let click = Vm::handler_info("ok_click").expect("ok_click");
        assert_eq!((click.sender, click.args, click.args_mut), (Some("Button"), Some("MouseEventArgs"), false));
        let key = Vm::handler_info("args_only").expect("args_only");
        assert_eq!((key.sender, key.args, key.args_mut), (None, Some("KeyEventArgs"), true));
        assert_eq!(Vm::handler_info("no_params").map(|h| (h.sender, h.args)), Some((None, None)));
        assert_eq!(Vm::handler_info("any_args").and_then(|h| h.args), Some("EventArgs"));
        assert_eq!(Vm::handler_info("untyped_sender").and_then(|h| h.sender), Some("*"));
        assert_eq!(Vm::handler_info("renamed").map(|h| h.method), Some("renamed_method"));
        assert!(Vm::handler_info("helper").is_none() && Vm::handler_info("new_vm").is_none());
        // The skipped method and the associated fn are still there.
        let mut vm = Vm::new_vm();
        assert_eq!(vm.helper(1), 2);
        vm.any_args(&EmptyEventArgs);
    }

    #[test]
    fn typed_args_and_typed_sender_reach_the_method() {
        let attrs = vec![("Text".to_string(), "Save".to_string())];
        let mut vm = Vm::default();
        let mut e = MouseEventArgs { button: MouseButton::Left, clicks: 1, x: 3.0, y: 4.0, ..Default::default() };
        assert!(dispatch_typed(&mut vm, "ok_click", &sender("Button", Some("Ok"), &attrs), &mut e));
        assert_eq!(vm.log, ["click Save Left 1 3,4"]);
    }

    #[test]
    fn sender_properties_resolve_bindings_against_the_view_model() {
        let attrs = vec![("IsOn".to_string(), "{Binding Status}".to_string())];
        let mut vm = Vm { status: "yes".into(), ..Default::default() };
        let mut e = CheckedChangedEventArgs::new(false, true, ChangeSource::User);
        assert!(dispatch_typed(&mut vm, "toggled", &sender("Switch", Some("Dark"), &attrs), &mut e));
        assert_eq!(vm.log, ["toggled yes -> true (Dark)"]);
    }

    #[test]
    fn handled_is_written_back_to_the_raiser() {
        let mut vm = Vm::default();
        let mut e = KeyEventArgs::new(Key::letter('s'), Modifiers::NONE);
        assert!(dispatch_typed(&mut vm, "args_only", &sender("TextField", None, &[]), &mut e));
        assert!(e.handled);
    }

    #[test]
    fn cancel_bridges_from_a_derived_cancelable_args() {
        let mut vm = Vm::default();
        let mut e = CancelEventArgs::default();
        assert!(dispatch_typed(&mut vm, "validating", &sender("TextField", None, &[]), &mut e));
        assert!(e.cancel);
        // A `&mut CancelEventArgs` handler bound to FormClosing (which extends CancelEventArgs).
        let mut closing = FormClosingEventArgs::default();
        assert!(dispatch_typed(&mut vm, "validating", &sender("Panel", None, &[]), &mut closing));
        assert!(closing.cancel);
    }

    #[test]
    fn base_and_dyn_args_accept_every_event() {
        let mut vm = Vm::default();
        let mut e = MouseEventArgs::default();
        assert!(dispatch_typed(&mut vm, "untyped_sender", &sender("Label", Some("Title"), &[]), &mut e));
        assert!(dispatch_typed(&mut vm, "any_args_mut", &sender("Label", None, &[]), &mut e));
        assert!(dispatch_typed(&mut vm, "no_params", &sender("Label", None, &[]), &mut e));
        assert!(dispatch_typed(&mut vm, "any_element", &sender("Label", None, &[]), &mut e));
        assert!(dispatch_typed(&mut vm, "renamed", &sender("Label", None, &[]), &mut e));
        assert_eq!(vm.log, ["from Title", "any MouseEventArgs", "no params", "any element Label", "renamed"]);
    }

    #[test]
    fn mismatched_args_or_sender_skip_the_handler_but_claim_the_name() {
        let mut vm = Vm::default();
        let mut key = KeyEventArgs::new(Key::letter('a'), Modifiers::NONE);
        assert!(dispatch_typed(&mut vm, "ok_click", &sender("Button", None, &[]), &mut key), "found, skipped");
        let mut mouse = MouseEventArgs::default();
        assert!(dispatch_typed(&mut vm, "ok_click", &sender("Switch", None, &[]), &mut mouse), "found, skipped");
        assert!(vm.log.is_empty(), "{:?}", vm.log);
        assert!(!dispatch_typed(&mut vm, "unknown", &sender("Button", None, &[]), &mut mouse));
        assert!(!dispatch_typed(&mut vm, "helper", &sender("Button", None, &[]), &mut mouse), "skipped methods are no handlers");
    }

    #[test]
    fn a_legacy_table_runs_behind_the_sink_with_the_legacy_value() {
        let mut vm = Vm::default();
        let mut table = handlers! {
            "legacy_toggled" => |vm, value| vm.set("Status", Value::Str(format!("{value:?}"))),
            "ok_click" => |vm, _v| vm.set("Status", Value::Str("legacy ran".into())),
        };
        let attrs = vec![("Text".to_string(), "Ok".to_string())];
        let s = sender("Button", None, &attrs);
        let mut typed = TypedViewModel(&mut vm);
        // The sink wins for a name it knows...
        assert!(table.dispatch_args("ok_click", &mut typed, &s, &mut MouseEventArgs::default()));
        // ...and the legacy entry gets exactly the legacy value otherwise.
        assert!(table.dispatch_args("legacy_toggled", &mut typed, &s, &mut CheckedChangedEventArgs::new(false, true, ChangeSource::User)));
        assert_eq!(vm.status, "Bool(true)");
        assert_eq!(vm.log.len(), 1);
        assert!(vm.log[0].starts_with("click Ok"));
    }

    #[test]
    fn a_plain_view_model_keeps_the_legacy_path() {
        let mut vm = MapViewModel::new();
        let mut table = handlers! { "go" => |vm, value| vm.set("Got", value) };
        assert!(table.dispatch_args("go", &mut vm, &ElementRef::detached("x"), &mut EmptyEventArgs));
        assert_eq!(vm.get("Got"), Some(Value::Bool(true)));
    }
}
