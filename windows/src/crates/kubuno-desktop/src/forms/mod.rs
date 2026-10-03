//! Controls and forms as objects, the Windows Forms way.
//!
//! A [`Control`] is a cheap, clonable handle (`Rc`) to one control of a form: an element of a
//! `.kbview` (the fields `#[kubuno_desktop::view]` generates), or a control created in code
//! (`Button::new().text("OK")`) and added to a form's [`ControlCollection`]. Setting a property
//! (`set_text`, `set_enabled`) shows at the next frame; reading one returns the current value,
//! including what the user typed. The typed handles ([`Button`], [`TextField`]…) deref to
//! [`Control`] and add their own properties, builder methods and events.
//!
//! How it works: a form's view is composed in memory — the `.kbview` text, with the properties of
//! its named controls bound to the controls' values and every event routed to the form, plus the
//! controls created in code — and handed to the `kubuno_desktop_views` runtime. Nothing is written to disk:
//! the `.kbview` stays the single source of truth. Changing a property the runtime reads through a
//! binding (`Text`, `Enabled`, `Visible`, `Checked`, `Value`…) costs nothing; changing another one
//! (`Variant`, the geometry) or adding a control recomposes the view for the next frame.

pub(crate) mod compose;
mod form;
mod types;
mod icon_source;
pub use icon_source::{IconScaling, IconSource};
/// The typed handles of the ribbon family.
mod ribbon;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use kubuno_desktop_views::binding::Value;
use kubuno_desktop_views::events::{
    ArgsChain, EmptyEventArgs, EventArgs, KeyEventArgs, KeyPressEventArgs, MouseEventArgs, TextChangedEventArgs,
};

pub use form::{AsForm, Form, HeaderItems};
pub(crate) use form::FormShared;
pub use kubuno_desktop_controls::host::{Backdrop, CornerPreference, FormBorderStyle, SizeGripStyle, StartPosition};
pub use kubuno_desktop_controls::window_chrome::{ButtonStyle as CaptionButtonStyle, CaptionCommand, TitleAlignment};
pub use kubuno_desktop_views::window::WindowKind;
pub use types::*;
pub use ribbon::*;

/// The edges of its container a control stays attached to when the container is resized
/// (Windows Forms' `AnchorStyles`). Combine them with `|`: `Anchor::TOP | Anchor::RIGHT`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Anchor(u8);

impl Anchor {
    pub const NONE: Self = Self(0);
    pub const TOP: Self = Self(1);
    pub const BOTTOM: Self = Self(2);
    pub const LEFT: Self = Self(4);
    pub const RIGHT: Self = Self(8);

    /// Whether every edge of `other` is in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The `.kbview` spelling: `"Top, Left"`, `"None"`.
    pub fn to_xml(self) -> String {
        let names = [(Self::TOP, "Top"), (Self::BOTTOM, "Bottom"), (Self::LEFT, "Left"), (Self::RIGHT, "Right")];
        let parts: Vec<&str> = names.iter().filter(|(a, _)| self.contains(*a)).map(|(_, n)| *n).collect();
        if parts.is_empty() { "None".to_string() } else { parts.join(", ") }
    }

    /// Reads the `.kbview` spelling (`"Top, Right"`); unknown words are ignored.
    pub fn parse(text: &str) -> Self {
        text.split([',', '|']).fold(Self::NONE, |acc, part| match part.trim() {
            "Top" => acc | Self::TOP,
            "Bottom" => acc | Self::BOTTOM,
            "Left" => acc | Self::LEFT,
            "Right" => acc | Self::RIGHT,
            _ => acc,
        })
    }
}

impl Default for Anchor {
    /// `Top, Left` (Windows Forms' default).
    fn default() -> Self {
        Self::TOP | Self::LEFT
    }
}

impl std::ops::BitOr for Anchor {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Anchor {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::fmt::Debug for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Anchor({})", self.to_xml())
    }
}

/// The edge of its container a control is docked to (Windows Forms' `DockStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockStyle {
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    Fill,
}

impl DockStyle {
    fn to_xml(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Fill => "Fill",
        }
    }
}

/// How a dialog was closed (Windows Forms' `DialogResult`): what [`View::show_dialog`](crate::View::show_dialog)
/// and [`MessageBox::show`](crate::MessageBox::show) return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum DialogResult {
    /// Still open, or closed without a result.
    #[default]
    None,
    Ok,
    Cancel,
    Abort,
    Retry,
    Ignore,
    Yes,
    No,
    TryAgain,
    Continue,
}

/// Anything that is a control: the typed handles and [`Control`] itself.
pub trait AsControl {
    fn as_control(&self) -> &Control;
}

impl AsControl for Control {
    fn as_control(&self) -> &Control {
        self
    }
}

/// The identity of a subscription made with [`ControlEvent::subscribe`], to remove it with
/// [`ControlEvent::unsubscribe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlerId(u64);

/// A Rust subscriber of one event of one control.
type Subscriber = Rc<RefCell<dyn FnMut(&Control, &Form, &mut dyn EventArgs)>>;

/// The geometry a control was given in code (or read from its `.kbview`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Layout {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub anchor: Option<Anchor>,
    pub dock: Option<DockStyle>,
}

pub(crate) struct ControlInner {
    /// The element (`"Button"`).
    pub element: RefCell<String>,
    /// Its `x:Name` (given, generated when it joins a form, or read from the view).
    pub name: RefCell<String>,
    /// The property values: the view's literals, what code set, what the user typed.
    pub props: RefCell<BTreeMap<String, Value>>,
    /// The properties code has set (they win over the view's literal at the next composition).
    pub code_set: RefCell<BTreeSet<String>>,
    /// The properties the composed view reads through a binding to this control (changing them
    /// needs no recomposition).
    pub bound: RefCell<BTreeSet<String>>,
    /// Properties the view binds to the form's own data (`Text="{Binding Status}"`): property →
    /// binding path. Their value is refreshed from the view model every frame.
    pub user_bound: RefCell<BTreeMap<String, String>>,
    /// Values code set on a `user_bound` property, written to the view model at the next frame.
    pub pending_writes: RefCell<Vec<(String, Value)>>,
    pub layout: Cell<Layout>,
    /// The geometry code set (wins over the view's).
    pub layout_set: Cell<bool>,
    /// Controls added in code inside this one (a panel's children).
    pub children: RefCell<Vec<Control>>,
    pub subscribers: RefCell<Vec<(u64, &'static str, Subscriber)>>,
    pub next_id: Cell<u64>,
    /// The form the control belongs to.
    pub form: RefCell<Weak<FormShared>>,
    /// `DialogResult` of a button: clicking it closes its modal form with that result.
    pub dialog_result: Cell<DialogResult>,
    /// The control comes from the `.kbview` (not created in code).
    pub from_view: Cell<bool>,
}

/// One control of a form — a clonable handle (`Rc`) to it. See the [module](self) documentation.
#[derive(Clone)]
pub struct Control(pub(crate) Rc<ControlInner>);

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} x:Name=\"{}\">", self.element(), self.get_name())
    }
}

impl Default for Control {
    /// A control whose element is not known yet: a field `#[kubuno_desktop::view]` generates for a custom
    /// control, which `initialize_component` links to its element.
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for Control {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Control {
    /// A new control of element `element` (`"Button"`, or a custom control's name), not yet in a
    /// form. The typed handles' `new()` are the usual way.
    pub fn new(element: &str) -> Self {
        Self(Rc::new(ControlInner {
            element: RefCell::new(element.to_string()),
            name: RefCell::new(String::new()),
            props: RefCell::new(BTreeMap::new()),
            code_set: RefCell::new(BTreeSet::new()),
            bound: RefCell::new(BTreeSet::new()),
            user_bound: RefCell::new(BTreeMap::new()),
            pending_writes: RefCell::new(Vec::new()),
            layout: Cell::new(Layout::default()),
            layout_set: Cell::new(false),
            children: RefCell::new(Vec::new()),
            subscribers: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            form: RefCell::new(Weak::new()),
            dialog_result: Cell::new(DialogResult::None),
            from_view: Cell::new(false),
        }))
    }

    /// The element it is (`"Button"`).
    pub fn element(&self) -> String {
        self.0.element.borrow().clone()
    }

    /// Whether it is an `element`, or a control class derived from it (a `RoundButton` extending
    /// `Button`).
    pub fn is(&self, element: &str) -> bool {
        let own = self.element();
        own == element || kubuno_desktop_views::registry::lookup(&own).is_some_and(|m| m.is_a(element))
    }

    /// Its name (`x:Name`).
    pub fn get_name(&self) -> String {
        self.0.name.borrow().clone()
    }

    /// Renames it (before it joins a form; a control of a `.kbview` keeps its `x:Name`).
    pub fn set_name(&self, name: &str) {
        *self.0.name.borrow_mut() = name.to_string();
        self.structure_changed();
    }

    /// The form it belongs to, once it is in one.
    pub fn form(&self) -> Option<Form> {
        self.0.form.borrow().upgrade().map(|shared| Form { shared })
    }

    /// Runs `f` on the instance of this control's class as a `T` — a custom control of the
    /// project or of a library (`#[derive(Component)]`, `#[derive(UserControl)]`), Windows Forms'
    /// `((MessageThread)thread).Append(…)`:
    /// `self.thread.with(|t: &mut MessageThread| t.append(message))`. `None` when the control is
    /// not a `T`, or cannot be reached now: before its window opens, or while it is busy (its own
    /// handler is running).
    pub fn with<T: kubuno_desktop_views::component::Component, R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.scope()?.with::<T, R>(&self.get_name(), f)
    }

    /// [`Self::with`], reading only.
    pub fn with_ref<T: kubuno_desktop_views::component::Component, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.scope()?.with_ref::<T, R>(&self.get_name(), f)
    }

    /// Opens the view's `<ContextMenu x:Name="menu">` below this control (WinForms
    /// `contextMenuStrip1.Show(control, …)`), at the next frame.
    pub fn show_context_menu(&self, menu: &str) {
        kubuno_desktop_views::window::show_context_menu(menu, kubuno_desktop_views::window::MenuAnchor::Element(self.get_name()));
    }

    /// The text colour (`ForeColor`): a theme token (`"TextSecondary"`) or a colour (`"#FF8800"`);
    /// empty for the parent's. Changing it costs no recomposition.
    pub fn set_fore_color(&self, color: &str) {
        self.set_property("ForeColor", Value::Str(color.to_string()));
    }

    /// The background colour (`BackColor`), like [`Self::set_fore_color`].
    pub fn set_back_color(&self, color: &str) {
        self.set_property("BackColor", Value::Str(color.to_string()));
    }

    /// The named components of the view the control is in: its form's open window, else the view
    /// whose frame is running.
    fn scope(&self) -> Option<kubuno_desktop_views::scope::ComponentScope> {
        self.form().and_then(|form| form.shared.scope.borrow().clone()).or_else(kubuno_desktop_views::scope::current)
    }

    // ── Properties ───────────────────────────────────────────────────────────────────────────

    /// The value of property `name` (`"Text"`, `"Enabled"`, `"Variant"`…), when it has one.
    pub fn get_property(&self, name: &str) -> Option<Value> {
        self.0.props.borrow().get(name).cloned()
    }

    /// Sets property `name` (any property of the element: `set_property("Variant", "Danger")`).
    pub fn set_property(&self, name: &str, value: impl Into<Value>) {
        let value = value.into();
        if self.0.props.borrow().get(name) == Some(&value) {
            return;
        }
        self.0.code_set.borrow_mut().insert(name.to_string());
        if self.0.user_bound.borrow().contains_key(name) {
            self.0.pending_writes.borrow_mut().push((name.to_string(), value.clone()));
        }
        self.0.props.borrow_mut().insert(name.to_string(), value);
        if self.0.bound.borrow().contains(name) || self.0.user_bound.borrow().contains_key(name) {
            self.value_changed();
        } else {
            self.structure_changed();
        }
    }

    /// A property as text (`""` when unset or not text).
    pub(crate) fn string(&self, name: &str) -> String {
        match self.get_property(name) {
            Some(Value::Str(s)) => s,
            Some(Value::F32(f)) => compose::number(f),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    }

    pub(crate) fn flag(&self, name: &str, default: bool) -> bool {
        match self.get_property(name) {
            Some(Value::Bool(b)) => b,
            Some(Value::Str(s)) => s.trim() == "true",
            _ => default,
        }
    }

    pub(crate) fn number(&self, name: &str) -> Option<f32> {
        match self.get_property(name) {
            Some(Value::F32(f)) => Some(f),
            Some(Value::Str(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Its text (`Text`): a button's caption, what is typed in a field.
    pub fn get_text(&self) -> String {
        self.string(self.text_property())
    }

    pub fn set_text(&self, text: impl Into<String>) {
        self.set_property(self.text_property(), Value::Str(text.into()));
    }

    /// The property that is this control's `Text` (Windows Forms' `Control.Text`): `Text`, or the
    /// caption of a control that has none (`Title` of a `GroupBox` or a `Card`, `Header`, `Label`).
    pub fn text_property(&self) -> &'static str {
        let element = self.element();
        match kubuno_desktop_views::registry::lookup(&element) {
            Some(meta) => ["Text", "Title", "Header", "Label"].into_iter().find(|p| meta.property(p).is_some()).unwrap_or("Text"),
            None => "Text",
        }
    }

    /// Whether it takes input (`Enabled`, `true` by default).
    pub fn is_enabled(&self) -> bool {
        self.flag("Enabled", true)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.set_property("Enabled", Value::Bool(enabled));
    }

    /// Whether it is shown (`Visible`, `true` by default).
    pub fn is_visible(&self) -> bool {
        self.flag("Visible", true)
    }

    pub fn set_visible(&self, visible: bool) {
        self.set_property("Visible", Value::Bool(visible));
    }

    /// Shows it (`Visible = true`).
    pub fn show(&self) {
        self.set_visible(true);
    }

    /// Hides it (`Visible = false`).
    pub fn hide(&self) {
        self.set_visible(false);
    }

    /// Its tooltip text (`ToolTip`).
    pub fn get_tool_tip(&self) -> String {
        self.string("ToolTip")
    }

    pub fn set_tool_tip(&self, text: impl Into<String>) {
        self.set_property("ToolTip", Value::Str(text.into()));
    }

    // ── Layout ───────────────────────────────────────────────────────────────────────────────

    /// Where it is in its container (`X`, `Y`), in DIP.
    pub fn get_location(&self) -> (f32, f32) {
        let l = self.0.layout.get();
        (l.x.unwrap_or(0.0), l.y.unwrap_or(0.0))
    }

    pub fn set_location(&self, x: f32, y: f32) {
        self.update_layout(|l| {
            l.x = Some(x);
            l.y = Some(y);
        });
    }

    /// Its size (`Width`, `Height`), in DIP (0 when the view does not set it).
    pub fn get_size(&self) -> (f32, f32) {
        let l = self.0.layout.get();
        (l.width.unwrap_or(0.0), l.height.unwrap_or(0.0))
    }

    pub fn set_size(&self, width: f32, height: f32) {
        self.update_layout(|l| {
            l.width = Some(width);
            l.height = Some(height);
        });
    }

    /// Location and size at once.
    pub fn set_bounds(&self, x: f32, y: f32, width: f32, height: f32) {
        self.update_layout(|l| {
            l.x = Some(x);
            l.y = Some(y);
            l.width = Some(width);
            l.height = Some(height);
        });
    }

    /// The edges of its container it follows (`Anchor`, `Top, Left` by default).
    pub fn get_anchor(&self) -> Anchor {
        self.0.layout.get().anchor.unwrap_or_default()
    }

    pub fn set_anchor(&self, anchor: Anchor) {
        self.update_layout(|l| l.anchor = Some(anchor));
    }

    /// The edge of its container it is docked to (`Dock`).
    pub fn get_dock(&self) -> DockStyle {
        self.0.layout.get().dock.unwrap_or_default()
    }

    pub fn set_dock(&self, dock: DockStyle) {
        self.update_layout(|l| l.dock = Some(dock));
    }

    fn update_layout(&self, f: impl FnOnce(&mut Layout)) {
        let mut layout = self.0.layout.get();
        f(&mut layout);
        self.0.layout.set(layout);
        self.0.layout_set.set(true);
        self.structure_changed();
    }

    // ── Children ─────────────────────────────────────────────────────────────────────────────

    /// The controls added in code inside this one (a panel's, a group box's).
    pub fn controls(&self) -> ControlCollection {
        ControlCollection { owner: self.clone() }
    }

    // ── Events ───────────────────────────────────────────────────────────────────────────────

    /// Event `event` (its `.kbview` attribute name, `"OnClick"`, `"OnSelectionChanged"`…) with
    /// the sender typed as `C` and the args as `A`: the generic form of [`click`](Self::click)
    /// and the other accessors.
    pub fn on<C: SenderParam, A: EventArgs + ArgsChain>(&self, event: &'static str) -> ControlEvent<'_, C, A> {
        ControlEvent { control: self, event, marker: PhantomData }
    }

    /// `Click` (a press and release, or Space/Enter on a button).
    pub fn click(&self) -> ControlEvent<'_, Control, MouseEventArgs> {
        self.on("OnClick")
    }

    pub fn double_click(&self) -> ControlEvent<'_, Control, MouseEventArgs> {
        self.on("OnDoubleClick")
    }

    pub fn mouse_down(&self) -> ControlEvent<'_, Control, MouseEventArgs> {
        self.on("OnMouseDown")
    }

    pub fn mouse_up(&self) -> ControlEvent<'_, Control, MouseEventArgs> {
        self.on("OnMouseUp")
    }

    pub fn mouse_move(&self) -> ControlEvent<'_, Control, MouseEventArgs> {
        self.on("OnMouseMove")
    }

    pub fn mouse_enter(&self) -> ControlEvent<'_, Control, EmptyEventArgs> {
        self.on("OnMouseEnter")
    }

    pub fn mouse_leave(&self) -> ControlEvent<'_, Control, EmptyEventArgs> {
        self.on("OnMouseLeave")
    }

    pub fn key_down(&self) -> ControlEvent<'_, Control, KeyEventArgs> {
        self.on("OnKeyDown")
    }

    pub fn key_up(&self) -> ControlEvent<'_, Control, KeyEventArgs> {
        self.on("OnKeyUp")
    }

    pub fn key_press(&self) -> ControlEvent<'_, Control, KeyPressEventArgs> {
        self.on("OnKeyPress")
    }

    pub fn got_focus(&self) -> ControlEvent<'_, Control, EmptyEventArgs> {
        self.on("OnGotFocus")
    }

    pub fn lost_focus(&self) -> ControlEvent<'_, Control, EmptyEventArgs> {
        self.on("OnLostFocus")
    }

    /// `TextChanged` (typed, or set by code).
    pub fn text_changed(&self) -> ControlEvent<'_, Control, TextChangedEventArgs> {
        self.on("OnTextChanged")
    }

    /// `DialogResult`: a button's click closes its modal form with this result.
    pub fn get_dialog_result(&self) -> DialogResult {
        self.0.dialog_result.get()
    }

    pub fn set_dialog_result(&self, result: DialogResult) {
        self.0.dialog_result.set(result);
        // Its Click is routed to the form from the next composition.
        self.structure_changed();
    }

    /// The canonical names of the events this control has Rust subscribers for.
    pub(crate) fn subscribed_events(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self.0.subscribers.borrow().iter().map(|(_, e, _)| *e).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Runs the Rust subscribers of `event`, in subscription order (a subscriber that sets
    /// `handled` stops the others). A subscriber added meanwhile runs from the next raise.
    pub(crate) fn raise(&self, event: &str, form: &Form, args: &mut dyn EventArgs) {
        let subs: Vec<Subscriber> = self.0.subscribers.borrow().iter().filter(|(_, e, _)| *e == event).map(|(_, _, s)| s.clone()).collect();
        for sub in subs {
            if args.as_handled().is_some_and(|h| h.handled()) {
                break;
            }
            match sub.try_borrow_mut() {
                Ok(mut f) => f(self, form, args),
                Err(_) => tracing::warn!("{} of `{}` raised again from its own handler: skipped", event, self.get_name()),
            }
        }
    }

    // ── Change tracking ─────────────────────────────────────────────────────────────────────

    /// A value the composed view reads through a binding changed: the next frame shows it.
    pub(crate) fn value_changed(&self) {
        if let Some(shared) = self.0.form.borrow().upgrade() {
            shared.changed.set(true);
        }
    }

    /// Something only a new composition shows changed (a control added, a literal property).
    pub(crate) fn structure_changed(&self) {
        if let Some(shared) = self.0.form.borrow().upgrade() {
            shared.dirty.set(true);
        }
    }

    /// Attaches the control (and the controls added inside it) to `form`.
    pub(crate) fn attach(&self, form: &Rc<FormShared>) {
        *self.0.form.borrow_mut() = Rc::downgrade(form);
        if self.get_name().is_empty() {
            let name = form.next_name(&self.element());
            *self.0.name.borrow_mut() = name;
        }
        for child in self.0.children.borrow().iter() {
            child.attach(form);
        }
    }

    pub(crate) fn detach(&self) {
        *self.0.form.borrow_mut() = Weak::new();
        for child in self.0.children.borrow().iter() {
            child.detach();
        }
    }
}

/// The controls of a form (its top level) or of a container control, in their order — Windows
/// Forms' `Control.ControlCollection`.
#[derive(Clone)]
pub struct ControlCollection {
    owner: Control,
}

impl ControlCollection {
    /// Adds `control` at the end. It gets a name (`button1`, `textField2`…) when it has none.
    pub fn add(&self, control: &impl AsControl) {
        let control = control.as_control().clone();
        if self.owner.0.children.borrow().contains(&control) {
            return;
        }
        if let Some(shared) = self.owner.0.form.borrow().upgrade() {
            control.attach(&shared);
        }
        self.owner.0.children.borrow_mut().push(control);
        self.owner.structure_changed();
    }

    /// Adds several controls.
    pub fn add_range(&self, controls: &[&dyn AsControl]) {
        for c in controls {
            let control = c.as_control().clone();
            self.add(&control);
        }
    }

    /// Removes `control`; `false` when it was not in this collection.
    pub fn remove(&self, control: &impl AsControl) -> bool {
        let control = control.as_control();
        let mut children = self.owner.0.children.borrow_mut();
        let Some(index) = children.iter().position(|c| c == control) else { return false };
        let removed = children.remove(index);
        drop(children);
        removed.detach();
        self.owner.structure_changed();
        true
    }

    /// Removes every control.
    pub fn clear(&self) {
        let removed = std::mem::take(&mut *self.owner.0.children.borrow_mut());
        for c in &removed {
            c.detach();
        }
        self.owner.structure_changed();
    }

    pub fn len(&self) -> usize {
        self.owner.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The control at `index`.
    pub fn get(&self, index: usize) -> Option<Control> {
        self.owner.0.children.borrow().get(index).cloned()
    }

    /// The control named `name`, searched in this collection and the collections of its controls.
    pub fn find(&self, name: &str) -> Option<Control> {
        for c in self.owner.0.children.borrow().iter() {
            if c.get_name() == name {
                return Some(c.clone());
            }
            if let Some(found) = c.controls().find(name) {
                return Some(found);
            }
        }
        None
    }

    pub fn contains(&self, control: &impl AsControl) -> bool {
        self.owner.0.children.borrow().iter().any(|c| c == control.as_control())
    }

    /// The controls, in order.
    pub fn to_vec(&self) -> Vec<Control> {
        self.owner.0.children.borrow().clone()
    }
}

/// What an event handler's `sender` can be: [`Control`] (any element), a typed handle
/// ([`Button`]: the event must come from a `<Button>`, or a class derived from it) or the
/// [`Form`] (any element of the form).
pub trait SenderParam: Sized + 'static {
    /// The sender for an event raised by `control` of `form`; `None` (the handler is skipped with
    /// a warning naming `handler`) when the control is not one.
    fn from_sender(control: &Control, form: &Form, handler: &str) -> Option<Self>;
}

impl SenderParam for Control {
    fn from_sender(control: &Control, _form: &Form, _handler: &str) -> Option<Self> {
        Some(control.clone())
    }
}

impl SenderParam for Form {
    fn from_sender(_control: &Control, form: &Form, _handler: &str) -> Option<Self> {
        Some(form.clone())
    }
}

/// One event of one control, for Rust subscribers: `ok.click().subscribe(|sender, e| …)`.
pub struct ControlEvent<'a, C, A> {
    control: &'a Control,
    event: &'static str,
    marker: PhantomData<fn(&C, &mut A)>,
}

impl<C: SenderParam, A: EventArgs + ArgsChain> ControlEvent<'_, C, A> {
    /// Runs `handler` each time the event is raised, after the handler the `.kbview` names (if
    /// any) — Windows Forms' `+=`. It stays subscribed until [`unsubscribe`](Self::unsubscribe)
    /// (or the control is dropped).
    pub fn subscribe(&self, mut handler: impl FnMut(&C, &mut A) + 'static) -> HandlerId {
        let event = self.event;
        let id = self.control.0.next_id.get();
        self.control.0.next_id.set(id + 1);
        let sub: Subscriber = Rc::new(RefCell::new(move |control: &Control, form: &Form, args: &mut dyn EventArgs| {
            let Some(sender) = C::from_sender(control, form, event) else { return };
            kubuno_desktop_views::events::typed::with_args::<A, ()>(args, event, |a| handler(&sender, a));
        }));
        let first = !self.control.0.subscribers.borrow().iter().any(|(_, e, _)| *e == event);
        self.control.0.subscribers.borrow_mut().push((id, event, sub));
        if first {
            // The composed view routes this event to the form from now on.
            self.control.structure_changed();
        }
        HandlerId(id)
    }

    /// Removes a subscription (Windows Forms' `-=`); `false` when it was not one of this event.
    pub fn unsubscribe(&self, id: HandlerId) -> bool {
        let mut subs = self.control.0.subscribers.borrow_mut();
        let before = subs.len();
        subs.retain(|(i, e, _)| !(*i == id.0 && *e == self.event));
        before != subs.len()
    }

    /// Whether the event has Rust subscribers.
    pub fn has_subscribers(&self) -> bool {
        self.control.0.subscribers.borrow().iter().any(|(_, e, _)| *e == self.event)
    }
}

/// The generated names of a form's controls (`button1`), per element.
pub(crate) type NameCounters = HashMap<String, u32>;

/// The source record of a form's view (see `__private::initialize_view`).
pub(crate) fn form_source(path: Option<&str>, text: &str, display: &str) -> form::Source {
    form::Source { path: path.map(str::to_string), text: text.to_string(), display: display.to_string() }
}
