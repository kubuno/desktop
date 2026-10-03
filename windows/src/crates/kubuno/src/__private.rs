//! What the code `#[kubuno::view]` generates calls. Not a public API: it may change with the macro.

use std::future::Future;

use kubuno_views::binding::Value;
use kubuno_views::events::typed::with_args;
use kubuno_views::events::{ArgsChain, ElementRef, EventArgs, ReadOnlyArgs, UiHandle};

use crate::forms::compose;
use crate::forms::{Control, DialogResult, Form, SenderParam};
use crate::View;

/// The view a struct with `#[kubuno::view]` stands for.
pub struct ViewSource {
    /// The `.kbview` file (absolute), for hot reload; `None` for an inline view.
    pub path: Option<&'static str>,
    /// Its text, embedded at compile time.
    pub text: &'static str,
    /// Its name in messages.
    pub display: &'static str,
}

/// `initialize_component()`: links the generated fields to the view's elements (by `x:Name`) and
/// reads the view's values into them.
pub fn initialize_view(form: &Form, source: ViewSource, members: &[(&str, &Control)]) {
    let shared = &form.shared;
    *shared.source.borrow_mut() = Some(crate::forms::form_source(source.path, source.text, source.display));
    {
        let mut map = shared.members.borrow_mut();
        for (name, control) in members {
            *control.0.name.borrow_mut() = (*name).to_string();
            control.0.from_view.set(true);
            *control.0.form.borrow_mut() = std::rc::Rc::downgrade(shared);
            map.insert((*name).to_string(), (*control).clone());
        }
    }
    // The controls know their element and the view's values from here on (before the window opens).
    let _ = compose::compose(form);
    shared.dirty.set(true);
}

/// `ViewModel::get` of a view: the value of a control property the composed view binds.
pub fn form_get(form: &Form, path: &str) -> Option<Value> {
    let (control, prop) = compose::resolve_path(form, path)?;
    control.get_property(&prop)
}

/// `ViewModel::set` of a view: a value the user entered in a control. `Some(value)` back when
/// `path` is not a control's (the view's own bindings then get it).
pub fn form_set(form: &Form, path: &str, value: Value) -> Option<Value> {
    let Some((control, prop)) = compose::resolve_path(form, path) else { return Some(value) };
    if control.0.props.borrow().get(&prop) != Some(&value) {
        control.0.props.borrow_mut().insert(prop, value);
        form.shared.changed.set(true);
    }
    None
}

/// What a handler's generated call receives besides the args.
pub struct HandlerCx<'a> {
    pub(crate) control: &'a Control,
    pub(crate) form: &'a Form,
}

impl<'a> HandlerCx<'a> {
    pub fn new(control: &'a Control, form: &'a Form) -> Self {
        Self { control, form }
    }

    /// The control that raised the event.
    pub fn control(&self) -> &Control {
        self.control
    }
}

/// `ViewModel::dispatch_event` of a view: an event of the composed view runs the handler its
/// `.kbview` names (a method of the view), then its control's Rust subscribers, then a button's
/// `DialogResult`.
pub fn dispatch<V: View>(view: &mut V, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
    let form = view.form().clone();
    let Some(entry) = compose::synthetic(&form, handler) else {
        // A view that did not parse is compiled as written: its handlers keep their own names.
        let control = sender.name.and_then(|n| form.control(n)).unwrap_or_else(|| Control::new(sender.element));
        let cx = HandlerCx::new(&control, &form);
        return view.handle_event(handler, &cx, args);
    };
    let cx = HandlerCx::new(&entry.control, &form);
    if let Some(user) = &entry.user {
        if !view.handle_event(user, &cx, args) {
            tracing::warn!("{} `{user}` of `{}` is not a method of the view (rebuild after adding it)", entry.event, entry.control.get_name());
        }
    }
    entry.control.raise(&entry.event, &form, args);
    if entry.event == "OnClick" {
        let result = entry.control.get_dialog_result();
        if result != DialogResult::None {
            form.set_dialog_result(result);
        }
    }
    true
}

/// A method of a view that handles an event — implemented for the accepted signatures:
///
/// | Signature | |
/// |---|---|
/// | `fn f(&mut self)` | any event |
/// | `fn f(&mut self, e: &A)` / `e: &mut A` | `A` an args type (`&mut` to set `handled` / `cancel`); `&EventArgs` (the root) for any event |
/// | `fn f(&mut self, sender: &S, e: &A)` / `e: &mut A` | `S`: `Control`, a typed control (`Button`…) or `Form` |
/// | `fn f(&mut self, sender: &S)` | |
/// | `async fn f(ui: UiHandle<Self>)` / `async fn f(ui: UiHandle<Self>, e: A)` | runs as a task on the UI thread; `e` is a copy |
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot handle an event of the view `{V}`",
    label = "not an event handler",
    note = "a handler is a method of the view: `fn name(&mut self)`, `fn name(&mut self, e: &MouseEventArgs)`, `fn name(&mut self, sender: &Button, e: &MouseEventArgs)` (`e: &mut …` to set `handled`/`cancel`), or `async fn name(ui: UiHandle<Self>, e: MouseEventArgs)`"
)]
pub trait Handler<V, Marker> {
    fn invoke(&mut self, view: &mut V, cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str);
}

/// Event args a handler takes by reference: an args type, or `dyn EventArgs`.
pub trait ArgsParam {
    fn with(args: &mut dyn EventArgs, name: &str, f: &mut dyn FnMut(&mut Self));
}

impl<T: EventArgs + ArgsChain> ArgsParam for T {
    fn with(args: &mut dyn EventArgs, name: &str, f: &mut dyn FnMut(&mut Self)) {
        with_args::<T, ()>(args, name, |a| f(a));
    }
}

impl ArgsParam for dyn EventArgs {
    fn with(args: &mut dyn EventArgs, _name: &str, f: &mut dyn FnMut(&mut Self)) {
        f(args)
    }
}

#[doc(hidden)]
pub struct NoArgs;
#[doc(hidden)]
pub struct Args<A: ?Sized>(std::marker::PhantomData<*const A>);
#[doc(hidden)]
pub struct ArgsMut<A: ?Sized>(std::marker::PhantomData<*const A>);
#[doc(hidden)]
pub struct SenderOnly<S>(std::marker::PhantomData<S>);
#[doc(hidden)]
pub struct SenderArgs<S, A: ?Sized>(std::marker::PhantomData<(S, *const A)>);
#[doc(hidden)]
pub struct SenderArgsMut<S, A: ?Sized>(std::marker::PhantomData<(S, *const A)>);
#[doc(hidden)]
pub struct AsyncNoArgs;
#[doc(hidden)]
pub struct AsyncArgs<A>(std::marker::PhantomData<A>);

impl<V, F: FnMut(&mut V)> Handler<V, NoArgs> for F {
    fn invoke(&mut self, view: &mut V, _cx: &HandlerCx<'_>, _args: &mut dyn EventArgs, _name: &str) {
        self(view)
    }
}

impl<V, A: ArgsParam + ?Sized, F: FnMut(&mut V, &A)> Handler<V, Args<A>> for F {
    fn invoke(&mut self, view: &mut V, _cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) {
        A::with(args, name, &mut |a| self(view, a))
    }
}

impl<V, A: ArgsParam + ?Sized, F: FnMut(&mut V, &mut A)> Handler<V, ArgsMut<A>> for F {
    fn invoke(&mut self, view: &mut V, _cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) {
        A::with(args, name, &mut |a| self(view, a))
    }
}

impl<V, S: SenderParam, F: FnMut(&mut V, &S)> Handler<V, SenderOnly<S>> for F {
    fn invoke(&mut self, view: &mut V, cx: &HandlerCx<'_>, _args: &mut dyn EventArgs, name: &str) {
        if let Some(sender) = S::from_sender(cx.control, cx.form, name) {
            self(view, &sender)
        }
    }
}

impl<V, S: SenderParam, A: ArgsParam + ?Sized, F: FnMut(&mut V, &S, &A)> Handler<V, SenderArgs<S, A>> for F {
    fn invoke(&mut self, view: &mut V, cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) {
        if let Some(sender) = S::from_sender(cx.control, cx.form, name) {
            A::with(args, name, &mut |a| self(view, &sender, a))
        }
    }
}

impl<V, S: SenderParam, A: ArgsParam + ?Sized, F: FnMut(&mut V, &S, &mut A)> Handler<V, SenderArgsMut<S, A>> for F {
    fn invoke(&mut self, view: &mut V, cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) {
        if let Some(sender) = S::from_sender(cx.control, cx.form, name) {
            A::with(args, name, &mut |a| self(view, &sender, a))
        }
    }
}

impl<V: 'static, Fut: Future<Output = ()> + 'static, F: FnMut(UiHandle<V>) -> Fut> Handler<V, AsyncNoArgs> for F {
    fn invoke(&mut self, _view: &mut V, _cx: &HandlerCx<'_>, _args: &mut dyn EventArgs, name: &str) {
        match UiHandle::<V>::current() {
            Some(ui) => drop(kubuno_views::events::spawn_local(self(ui))),
            None => tracing::warn!("async handler `{name}` needs a running view"),
        }
    }
}

impl<V: 'static, A: EventArgs + ArgsChain + ReadOnlyArgs + Clone, Fut: Future<Output = ()> + 'static, F: FnMut(UiHandle<V>, A) -> Fut> Handler<V, AsyncArgs<A>> for F {
    fn invoke(&mut self, _view: &mut V, _cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) {
        let Some(copy) = with_args::<A, A>(args, name, |a| a.clone()) else { return };
        match UiHandle::<V>::current() {
            Some(ui) => drop(kubuno_views::events::spawn_local(self(ui, copy))),
            None => tracing::warn!("async handler `{name}` needs a running view"),
        }
    }
}

/// Calls `handler` (a method of the view) for an event; the arm of the generated `match`.
pub fn call_handler<V, M, H: Handler<V, M>>(view: &mut V, mut handler: H, cx: &HandlerCx<'_>, args: &mut dyn EventArgs, name: &str) -> bool {
    handler.invoke(view, cx, args, name);
    true
}

/// The composed view of `form`, as the runtime gets it (tests, diagnostics).
pub fn compose_text(form: &Form) -> String {
    compose::compose(form).text
}

/// Marks `form` as open as a modal dialog (tests of `DialogResult` without a window).
pub fn set_modal(form: &Form, modal: bool) {
    form.shared.modal.set(modal);
}

/// Whether `form` was asked to close (and has not closed yet).
pub fn close_requested(form: &Form) -> bool {
    let request = form.shared.close_request.get();
    request.is_some()
}

/// The element of the typed handle named `name` (`kubuno::forms::Button` → `"Button"`), when this
/// crate has one — what the `#[kubuno::view]` table `TYPED_CONTROLS` is checked against.
pub fn typed_handle_element(name: &str) -> Option<&'static str> {
    macro_rules! elements {
        ($($t:ident),*) => { [$(crate::forms::$t::ELEMENT),*] };
    }
    let all = elements!(
        Accordion, Badge, Breadcrumb, Button, Callout, Card, CheckBox, CheckedListBox, ColorField, ComboBox, GradientField, DataTable, DatePicker, Dropdown, EmptyState, GroupBox, Icon,
        IconButton, Label, LinkLabel, ListBox, ListView, MaskedField, MonthCalendar, NumericField, PaintBox, Panel, ProgressBar, RadioButton, ScrollArea, SearchField,
        Separator, Slider, Spinner, Splitter, Stack, Stepper, Switch, Tabs, TextArea, TextField, Toolbar, TreeView, DockArea, WorkspaceShell, Avatar, PictureBox, Popover,
        Repeater, Sidebar, StatusBar, TableLayoutPanel, Ribbon, RibbonTab, RibbonContextualTabGroup, RibbonGroup, RibbonControlGroup, RibbonBox,
        RibbonQuickAccessToolbar, RibbonBackstage, BackstageTab, BackstageButton, BackstageSeparator, RibbonButton, RibbonToggleButton, RibbonRadioButton,
        RibbonMenuButton, RibbonSplitButton, RibbonColorPicker, RibbonMenuItem, RibbonSplitMenuItem, RibbonCheckBox, RibbonComboBox, RibbonTextBox,
        RibbonNumericField, RibbonGallery, RibbonGalleryCategory, RibbonGalleryItem, RibbonLabel, RibbonSeparator, Command
    );
    all.into_iter().find(|e| *e == name)
}
