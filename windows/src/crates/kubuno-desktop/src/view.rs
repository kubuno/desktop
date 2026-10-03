//! [`View`]: what a window shows — a form class (`#[kubuno_desktop::view]`) or a [`Form`] built in code.

use kubuno_desktop_views::binding::{Value, ViewModel};
use kubuno_desktop_views::events::{ElementRef, EventArgs, UiDispatcher};

use crate::__private::HandlerCx;
use crate::forms::{AsForm, DialogResult, Form};

/// What a window shows: a [`Form`] and the code that handles its events — Windows Forms' `Form`
/// subclass.
///
/// `#[kubuno_desktop::view("main_view.kbview")]` on a struct implements it: the struct gets a field per
/// `x:Name`d control of the view (typed with its handle: `status: TextField`), a hidden [`Form`] it
/// derefs to (`self.set_text("Title")`, `self.close()`, `self.controls()`), `initialize_component()`,
/// `Default` (unless derived), and each handler the view names (`OnClick="hello_click"`) runs the
/// struct's method of that name — a plain method, with one of the signatures of
/// [`__private::Handler`](crate::__private::Handler):
///
/// ```ignore
/// fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs) { … }
/// ```
///
/// A [`Form`] built in code is a `View` too (its events have Rust subscribers only).
///
/// Mistakes are compile errors that name the `.kbview` line — a control the crate does not know:
///
/// ```compile_fail
/// #[kubuno_desktop::view(xml = r#"<Panel><Frobnicator x:Name="f"/></Panel>"#)]
/// pub struct V;
/// ```
///
/// an `x:Name` that is not a Rust identifier, or two controls with one name:
///
/// ```compile_fail
/// #[kubuno_desktop::view(xml = r#"<Panel><Button x:Name="my-button"/></Panel>"#)]
/// pub struct V;
/// ```
///
/// ```compile_fail
/// #[kubuno_desktop::view(xml = r#"<Panel><Button x:Name="ok"/><Label x:Name="ok"/></Panel>"#)]
/// pub struct V;
/// ```
///
/// a handler the view names that the struct does not have (E0599), or one whose signature is not a
/// handler's (E0277, "`…` cannot handle an event of the view `V`"):
///
/// ```compile_fail,E0599
/// #[kubuno_desktop::view(xml = r#"<Panel><Button x:Name="ok" OnClick="ok_click"/></Panel>"#)]
/// pub struct V;
/// ```
///
/// ```compile_fail,E0277
/// use kubuno_desktop::prelude::*;
/// #[kubuno_desktop::view(xml = r#"<Panel><Button x:Name="ok" OnClick="ok_click"/></Panel>"#)]
/// pub struct V;
/// impl V {
///     fn ok_click(&mut self, count: u32) {}
/// }
/// ```
///
/// ```compile_fail,E0277
/// use kubuno_desktop::prelude::*;
/// #[kubuno_desktop::view(xml = r#"<Panel><Button x:Name="ok" OnClick="ok_click"/></Panel>"#)]
/// pub struct V;
/// impl V {
///     // The sender comes before the args.
///     fn ok_click(&mut self, e: &MouseEventArgs, sender: &Button) {}
/// }
/// ```
///
/// The accepted shapes compile:
///
/// ```
/// use kubuno_desktop::prelude::*;
/// #[kubuno_desktop::view(xml = r#"<Panel OnLoad="load"><Button x:Name="ok" OnClick="a" OnMouseDown="b" OnKeyDown="c" OnGotFocus="d" OnMouseUp="e" OnDoubleClick="f"/></Panel>"#)]
/// pub struct V;
/// impl V {
///     fn load(&mut self, sender: &Form, e: &EventArgs) {}
///     fn a(&mut self) {}
///     fn b(&mut self, e: &MouseEventArgs) {}
///     fn c(&mut self, sender: &Button, e: &mut KeyEventArgs) { e.handled = true; }
///     fn d(&mut self, sender: &Control) {}
///     fn e(&mut self, sender: &Button, e: &dyn kubuno_desktop::events::EventArgs) {}
///     async fn f(ui: UiHandle<Self>, e: MouseEventArgs) {}
/// }
/// ```
pub trait View: ViewModel + 'static {
    /// The handler names the view uses (diagnostics).
    const HANDLERS: &'static [&'static str] = &[];

    /// The view's form.
    fn form(&self) -> &Form;

    /// Runs the handler method named `handler`; `false` when the view has none of that name.
    fn handle_event(&mut self, handler: &str, cx: &HandlerCx<'_>, args: &mut dyn EventArgs) -> bool {
        let _ = (handler, cx, args);
        false
    }

    /// Opens the view in a window of its own and returns at once — Windows Forms' `Show()`. The
    /// window lives until it is closed, or until the application's main window closes. Call it from
    /// a running application (an event handler).
    fn show(self)
    where
        Self: Sized,
    {
        crate::application::show(self);
    }

    /// Opens the view as a modal dialog owned by `owner` (the other windows of the application do
    /// not take input meanwhile) and returns how it was closed once it is — Windows Forms'
    /// `ShowDialog(owner)`. The view stays usable afterwards: read its controls, its fields.
    fn show_dialog(&mut self, owner: &dyn AsForm) -> DialogResult
    where
        Self: Sized,
    {
        crate::application::show_dialog(self, owner.as_form().handle())
    }

    /// Opens the view as a dialog drawn INSIDE `owner`'s window, over a veil (the web's modal
    /// `FloatingWindow`, a ContentDialog): the page underneath takes no input until it closes, and
    /// `on_closed` then receives how it was closed. Returns at once (the owner's frame goes on).
    fn show_in_window(self, owner: &dyn AsForm, on_closed: impl FnOnce(DialogResult) + 'static)
    where
        Self: Sized,
    {
        crate::application::open_inner(owner.as_form(), self, true, Some(Box::new(on_closed)));
    }

    /// Opens the view as a flyout (`WindowKind::Flyout`) at `(x, y)` on screen (DIP): borderless,
    /// rounded, Acrylic; it closes when it loses the focus (a click outside).
    fn show_flyout(self, x: f32, y: f32)
    where
        Self: Sized,
    {
        let form = self.form();
        form.set_window_kind(crate::forms::WindowKind::Flyout);
        form.set_start_position(crate::forms::StartPosition::Manual);
        form.root().set_property("X", x);
        form.root().set_property("Y", y);
        crate::application::show(self);
    }

    /// A handle that posts closures to the view's UI thread from any thread (Windows Forms'
    /// `BeginInvoke`/`Invoke`), while its window is open.
    fn dispatcher(&self) -> Option<UiDispatcher<Self>>
    where
        Self: Sized,
    {
        self.form().shared.dispatcher.borrow().as_ref().and_then(|d| d.downcast_ref::<UiDispatcher<Self>>()).cloned()
    }
}

impl ViewModel for Form {
    fn get(&self, path: &str) -> Option<Value> {
        crate::__private::form_get(self, path)
    }

    fn set(&mut self, path: &str, value: Value) {
        let _ = crate::__private::form_set(self, path, value);
    }

    fn dispatch_event(&mut self, handler: &str, sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        crate::__private::dispatch(self, handler, sender, args)
    }
}

impl View for Form {
    fn form(&self) -> &Form {
        self
    }
}
