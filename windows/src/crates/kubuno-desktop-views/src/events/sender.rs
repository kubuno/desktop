//! The sender of an event (`object sender` in WinForms) and the deferred
//! command queue handlers use to act on controls (`vskubuno/docs/EVENTS.md`
//! §1, "Sender").
//!
//! The node raising an event is mutably borrowed while it does, and in Kubuno
//! the state lives in the view model anyway, so the sender is a **read-only**
//! borrowed handle: [`ElementRef`] (who raised it) and [`Sender<C>`] (the
//! same, plus the element's resolved properties of this frame, so
//! `sender.text()` covers WinForms' `((Button)sender).Text`). Imperative
//! actions (`focus()`, `select_all()`, a property override) are *queued* on a
//! [`ControlQueue`] and applied by the runtime right after the dispatch that
//! produced them, never mid-traversal.

use std::fmt;
use std::ops::Deref;

use kubuno_desktop_ui::{FocusId, Rect};

use crate::binding::Value;

/// An element's stable identity: its child-ordinal path in the document
/// (`"2.0.3"`, `""` for the root), the id the design surface and the language
/// server already agree on (`vskubuno/docs/DESIGNER.md` §8).
pub type ElementId = str;

/// Who raised an event: a borrowed, copyable description of the element.
///
/// ```
/// use kubuno_desktop_views::events::ElementRef;
/// use kubuno_desktop_ui::Rect;
///
/// let ok = ElementRef { name: Some("Ok"), element: "Button", id: "1.0",
///                       bounds: Rect::new(10.0, 20.0, 110.0, 52.0), focus_id: None, attributes: &[] };
/// assert_eq!(ok.to_local(15.0, 30.0), (5.0, 10.0));
/// assert!(ok.contains(50.0, 40.0) && !ok.contains(5.0, 40.0));
/// assert_eq!(ok.display_name(), "Ok");
/// ```
#[derive(Clone, Copy)]
pub struct ElementRef<'a> {
    /// The element's `x:Name`, if it has one.
    pub name: Option<&'a str>,
    /// The element name (`"Button"`), empty for a sender that is no element.
    pub element: &'static str,
    /// The stable path ([`ElementId`]).
    pub id: &'a ElementId,
    /// Where the element was painted this frame, in window DIP.
    pub bounds: Rect,
    /// Its focus identity, when it is focusable.
    pub focus_id: Option<FocusId>,
    /// The element's non-event XML attributes as written (`("Text", "Save")`,
    /// `("IsChecked", "{Binding Agree}")`): what [`ElementProps::resolve`] turns into the
    /// properties a typed [`Sender`] exposes. Empty for a detached sender.
    pub attributes: &'a [(String, String)],
}

impl<'a> ElementRef<'a> {
    /// A sender that is no element of a view — a view model, a service, a
    /// test — named `name` (empty element name and id, empty bounds).
    pub fn detached(name: &'a str) -> Self {
        Self { name: Some(name), element: "", id: "", bounds: Rect::default(), focus_id: None, attributes: &[] }
    }

    /// Window DIP → coordinates relative to the element's top-left (what
    /// mouse args carry).
    pub fn to_local(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.bounds.left, y - self.bounds.top)
    }

    /// Whether the window-DIP point lies inside the bounds (right and bottom
    /// edges excluded).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let b = &self.bounds;
        x >= b.left && x < b.right && y >= b.top && y < b.bottom
    }

    /// `x:Name`, else the element name, else the id — for logs and messages.
    pub fn display_name(&self) -> &'a str {
        match self.name {
            Some(n) => n,
            None if !self.element.is_empty() => self.element,
            None => self.id,
        }
    }
}

impl fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.bounds;
        f.debug_struct("ElementRef")
            .field("name", &self.name)
            .field("element", &self.element)
            .field("id", &self.id)
            .field("bounds", &(b.left, b.top, b.right, b.bottom))
            .field("focus_id", &self.focus_id)
            .finish()
    }
}

/// An element's properties as resolved for the current frame: every non-event XML
/// attribute, a `{Binding Path}` replaced by the view model's current value at `Path`
/// (the text as written when the path is unset), a literal kept as its text. What a
/// [`Sender`] of a built-in control ([`crate::controls`]) derefs to, so
/// `sender.text()` reads the button's text (WinForms' `((Button)sender).Text`).
///
/// ```
/// use kubuno_desktop_views::binding::{MapViewModel, Value};
/// use kubuno_desktop_views::events::ElementProps;
///
/// let attrs = vec![("Text".to_string(), "Save".to_string()),
///                  ("IsChecked".to_string(), "{Binding Agree}".to_string()),
///                  ("Width".to_string(), "120".to_string())];
/// let vm = MapViewModel::new().with("Agree", Value::Bool(true));
/// let props = ElementProps::resolve(Some("SaveButton"), &attrs, &vm);
/// assert_eq!(props.text(), "Save");
/// assert_eq!(props.bool("IsChecked"), Some(true));
/// assert_eq!(props.f32("Width"), Some(120.0));
/// assert_eq!(props.name(), Some("SaveButton"));
/// assert!(props.get("Missing").is_none());
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementProps {
    name: Option<String>,
    values: Vec<(String, Value)>,
}

impl ElementProps {
    /// No properties (a sender that is no element).
    pub const fn empty() -> Self {
        Self { name: None, values: Vec::new() }
    }

    /// Resolves `attributes` (see [`ElementRef::attributes`]) against `vm`.
    pub fn resolve(name: Option<&str>, attributes: &[(String, String)], vm: &dyn crate::binding::ViewModel) -> Self {
        let values = attributes
            .iter()
            .map(|(attr, raw)| {
                let value = match crate::binding::parse_binding(raw) {
                    Some(spec) => vm.get(&spec.path).unwrap_or_else(|| Value::Str(raw.clone())),
                    None => Value::Str(raw.clone()),
                };
                (attr.clone(), value)
            })
            .collect();
        Self { name: name.map(str::to_string), values }
    }

    /// The element's `x:Name`.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The property named `name` (its XML attribute name), if the element sets it.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// The property as text (`Str` as is, a number or a bool formatted), if set.
    pub fn string(&self, name: &str) -> Option<String> {
        match self.get(name)? {
            Value::Str(s) => Some(s.clone()),
            Value::F32(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::List(_) | Value::Object(_) => None,
        }
    }

    /// The property as a bool (`Bool`, or the literal text `true`/`false`).
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(b) => Some(*b),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// The property as a number (`F32`, or a literal that parses as one).
    pub fn f32(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Value::F32(f) => Some(*f),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// The `Text` property (`""` when the element sets none) — `((Button)sender).Text`.
    pub fn text(&self) -> String {
        self.string("Text").unwrap_or_default()
    }

    /// Every resolved property, in document order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(n, v)| (n.as_str(), v))
    }
}

/// The sender type that accepts any element (`Sender<AnyElement>`): for a handler shared by
/// several kinds of controls, like a WinForms handler that never casts `sender`.
#[derive(Debug, Clone, Copy)]
pub struct AnyElement;

impl ElementType for AnyElement {
    const ELEMENT: &'static str = "*";
    type Resolved = ElementProps;
}

/// A control type a [`Sender`] can be typed with: every class of the control hierarchy
/// (`#[derive(Component)]` implements it — [`crate::controls::Button`], a custom
/// `RoundButton`…) and [`AnyElement`].
///
/// Minimal on purpose: only what `Sender<C>` needs. It was named `Component` before EVT-7a,
/// which gave that name to the root of the control hierarchy ([`crate::component::Component`]).
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a control a `Sender` can be typed with",
    label = "not a control class",
    note = "use the element's control type from `kubuno_desktop_views::controls` (`Sender<Button>`), `Sender<AnyElement>` or `&ElementRef` for any element"
)]
pub trait ElementType: 'static {
    /// The element name (`"Button"`).
    const ELEMENT: &'static str;
    /// The component's property values as resolved for the current frame.
    type Resolved;
}

/// A typed, read-only sender: the [`ElementRef`] plus the element's
/// resolved properties, reachable directly through `Deref`.
///
/// ```
/// use kubuno_desktop_views::events::{ElementRef, ElementType, Sender};
///
/// struct Button;
/// struct ButtonProps { text: String }
/// impl ButtonProps { fn text(&self) -> &str { &self.text } }
/// impl ElementType for Button { const ELEMENT: &'static str = "Button"; type Resolved = ButtonProps; }
///
/// let props = ButtonProps { text: "Save".into() };
/// let sender: Sender<'_, Button> = Sender::new(ElementRef::detached("SaveButton"), &props);
/// assert_eq!(sender.text(), "Save");               // ((Button)sender).Text
/// assert_eq!(sender.element().name, Some("SaveButton"));
/// ```
pub struct Sender<'a, C: ElementType> {
    inner: ElementRef<'a>,
    props: &'a C::Resolved,
}

impl<'a, C: ElementType> Sender<'a, C> {
    pub fn new(inner: ElementRef<'a>, props: &'a C::Resolved) -> Self {
        Self { inner, props }
    }

    /// Who raised the event.
    pub fn element(&self) -> &ElementRef<'a> {
        &self.inner
    }

    /// The resolved properties (also reachable through `Deref`).
    pub fn props(&self) -> &'a C::Resolved {
        self.props
    }
}

impl<C: ElementType> Clone for Sender<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: ElementType> Copy for Sender<'_, C> {}

impl<C: ElementType> Deref for Sender<'_, C> {
    type Target = C::Resolved;
    fn deref(&self) -> &C::Resolved {
        self.props
    }
}

impl<C: ElementType> fmt::Debug for Sender<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sender").field("component", &C::ELEMENT).field("element", &self.inner).finish()
    }
}

/// Which control a [`ControlCommand`] is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ControlTarget {
    /// By `x:Name` (survives hot reloads that move the element).
    Name(String),
    /// By stable path ([`ElementId`]).
    Id(String),
}

impl ControlTarget {
    /// The control named `name` (`x:Name`).
    pub fn name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }
}

impl From<&ElementRef<'_>> for ControlTarget {
    /// The element itself: by `x:Name` when it has one, else by path.
    fn from(e: &ElementRef<'_>) -> Self {
        match e.name {
            Some(n) => Self::Name(n.to_string()),
            None => Self::Id(e.id.to_string()),
        }
    }
}

impl From<&str> for ControlTarget {
    /// A bare string names a control by `x:Name`.
    fn from(name: &str) -> Self {
        Self::Name(name.to_string())
    }
}

/// An imperative action on a control, queued during a dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlCommand {
    /// Move the keyboard focus to the control (`Control.Focus()`).
    Focus(ControlTarget),
    /// Select all of a text control's content (`TextBox.SelectAll()`).
    SelectAll(ControlTarget),
    /// Override a property's value from code (`ChangeSource::Code`).
    SetProperty { target: ControlTarget, property: String, value: Value },
    /// Drop a previous override: the property follows its XML value or binding again.
    ClearProperty { target: ControlTarget, property: String },
}

/// The deferred command queue handlers act on controls through
/// (`cx.controls()`). The runtime drains it right after the dispatch that
/// filled it, in order.
///
/// ```
/// use kubuno_desktop_views::events::{ControlCommand, ControlQueue, ControlTarget, ElementRef};
/// use kubuno_desktop_views::binding::Value;
///
/// let mut q = ControlQueue::new();
/// q.focus("NameField").select_all("NameField");
/// q.set_property(&ElementRef::detached("Status"), "Text", Value::Str("Saved".into()));
/// let cmds: Vec<_> = q.drain().collect();
/// assert_eq!(cmds[0], ControlCommand::Focus(ControlTarget::name("NameField")));
/// assert_eq!(cmds.len(), 3);
/// assert!(q.is_empty());
/// ```
#[derive(Debug, Default, Clone)]
pub struct ControlQueue {
    commands: Vec<ControlCommand>,
}

impl ControlQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues any command.
    pub fn push(&mut self, command: ControlCommand) -> &mut Self {
        self.commands.push(command);
        self
    }

    /// Queues [`ControlCommand::Focus`].
    pub fn focus(&mut self, target: impl Into<ControlTarget>) -> &mut Self {
        self.push(ControlCommand::Focus(target.into()))
    }

    /// Queues [`ControlCommand::SelectAll`].
    pub fn select_all(&mut self, target: impl Into<ControlTarget>) -> &mut Self {
        self.push(ControlCommand::SelectAll(target.into()))
    }

    /// Queues [`ControlCommand::SetProperty`].
    pub fn set_property(&mut self, target: impl Into<ControlTarget>, property: impl Into<String>, value: Value) -> &mut Self {
        self.push(ControlCommand::SetProperty { target: target.into(), property: property.into(), value })
    }

    /// Queues [`ControlCommand::ClearProperty`].
    pub fn clear_property(&mut self, target: impl Into<ControlTarget>, property: impl Into<String>) -> &mut Self {
        self.push(ControlCommand::ClearProperty { target: target.into(), property: property.into() })
    }

    /// The queued commands, in order, without removing them.
    pub fn commands(&self) -> &[ControlCommand] {
        &self.commands
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Removes and yields every queued command, in order.
    pub fn drain(&mut self) -> std::vec::Drain<'_, ControlCommand> {
        self.commands.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_ref_helpers() {
        let e = ElementRef { name: None, element: "Button", id: "0.1", bounds: Rect::new(0.0, 0.0, 10.0, 10.0), focus_id: Some(FocusId(7)), attributes: &[] };
        assert_eq!(e.display_name(), "Button");
        assert!(e.contains(0.0, 0.0) && !e.contains(10.0, 5.0));
        let anon = ElementRef { name: None, element: "", id: "3", bounds: Rect::default(), focus_id: None, attributes: &[] };
        assert_eq!(anon.display_name(), "3");
        assert!(format!("{e:?}").contains("Button"));
    }

    #[test]
    fn control_target_from_element_prefers_name() {
        let named = ElementRef::detached("Ok");
        assert_eq!(ControlTarget::from(&named), ControlTarget::Name("Ok".into()));
        let unnamed = ElementRef { name: None, element: "Label", id: "2.1", bounds: Rect::default(), focus_id: None, attributes: &[] };
        assert_eq!(ControlTarget::from(&unnamed), ControlTarget::Id("2.1".into()));
    }

    #[test]
    fn queue_keeps_order_and_drains() {
        let mut q = ControlQueue::new();
        q.focus("A").clear_property("B", "Text").push(ControlCommand::SelectAll(ControlTarget::Id("1".into())));
        assert_eq!(q.len(), 3);
        assert!(matches!(q.commands()[1], ControlCommand::ClearProperty { .. }));
        let drained: Vec<_> = q.drain().collect();
        assert_eq!(drained[2], ControlCommand::SelectAll(ControlTarget::Id("1".into())));
        assert!(q.is_empty());
    }

    #[test]
    fn typed_sender_derefs_to_props() {
        struct Check;
        struct CheckProps {
            checked: bool,
        }
        impl ElementType for Check {
            const ELEMENT: &'static str = "CheckBox";
            type Resolved = CheckProps;
        }
        let props = CheckProps { checked: true };
        let s: Sender<'_, Check> = Sender::new(ElementRef::detached("Agree"), &props);
        let copy = s;
        assert!(copy.checked && s.props().checked);
        assert!(format!("{s:?}").contains("CheckBox"));
    }
}
