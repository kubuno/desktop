//! The value side of a control's declared properties and events (EVT-7b of
//! `vskubuno/docs/EVENTS.md`): [`PropertyValue`], the conversion between a `#[property]` field
//! and the XML/binding [`Value`], and [`raise_declared_event`], what a declared event's
//! generated `raise_…` method calls.

use crate::binding::Value;
use crate::events::{ElementRef, Event, EventArgs};
use crate::registry::PropKind;

use super::Component;

/// A type a control's `#[property]` field can have: how an XML attribute or a bound value becomes
/// it, how it reads back (a user control's bindings), and the kind the tools validate and
/// complete it as. Implemented for `bool`, the integer and float types, `String` and
/// `Option<String>`; `#[derive(PropertyValue)]` implements it for a fieldless enum.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be the type of a `#[property]`",
    label = "not a property value type",
    note = "use `bool`, a number, `String`, `Option<String>`, `Rows` (a list) or `Shared<T>` (any Rust value), derive `PropertyValue` on a fieldless enum, or implement `kubuno_views::component::PropertyValue` for it"
)]
pub trait PropertyValue: Sized {
    /// What the Properties window, the validator and completion expect.
    const KIND: PropKind;
    /// The value an attribute's text or a binding's value converts to, `None` when it does not.
    fn from_value(value: &Value) -> Option<Self>;
    /// The field as a binding value.
    fn to_value(&self) -> Value;
    /// The Properties window editor the type asks for: `"list"` ([`crate::binding::Rows`]) and
    /// `"object"` ([`Shared`]) mark a property set with a binding only.
    const EDITOR: Option<&'static str> = None;
}

impl PropertyValue for crate::binding::Rows {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("list");
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(rows) => Some(rows.clone()),
            _ => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::List(self.clone())
    }
}

/// A property of a custom control holding any Rust value — `#[property] messages:
/// Shared<Vec<Message>>`, set in the view with a binding (`Messages="{Binding Messages}"`) to a
/// view model path answering [`Value::Object`] (`Value::from(Shared::new(messages))`). Shared
/// (`Arc`), so handing it to the control every frame copies a pointer; it derefs to the value.
#[derive(Debug, Default)]
pub struct Shared<T>(std::sync::Arc<T>);

impl<T> Shared<T> {
    /// `value`, shared.
    pub fn new(value: T) -> Self {
        Self(std::sync::Arc::new(value))
    }

    /// The shared value itself.
    pub fn arc(&self) -> &std::sync::Arc<T> {
        &self.0
    }

    /// Whether `other` shares this very value.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> From<std::sync::Arc<T>> for Shared<T> {
    fn from(value: std::sync::Arc<T>) -> Self {
        Self(value)
    }
}

impl<T: std::any::Any + Send + Sync> From<Shared<T>> for Value {
    fn from(value: Shared<T>) -> Self {
        Value::Object(crate::binding::ObjectValue::from_arc(value.0))
    }
}

impl<T: std::any::Any + Send + Sync> PropertyValue for Shared<T> {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("object");
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Object(o) => o.downcast_arc::<T>().map(Self),
            _ => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::Object(crate::binding::ObjectValue::from_arc(self.0.clone()))
    }
}

impl PropertyValue for bool {
    const KIND: PropKind = PropKind::Bool;
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(*b),
            Value::Str(s) => match s.trim() {
                "true" | "True" => Some(true),
                "false" | "False" => Some(false),
                _ => None,
            },
            Value::F32(f) => Some(*f != 0.0),
            Value::List(_) | Value::Object(_) => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::F32(f) => Some(f64::from(*f)),
        Value::Str(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::List(_) | Value::Object(_) => None,
    }
}

macro_rules! float_values {
    ($($t:ty),*) => {$(
        impl PropertyValue for $t {
            const KIND: PropKind = PropKind::F32;
            fn from_value(value: &Value) -> Option<Self> {
                number(value).map(|n| n as $t)
            }
            fn to_value(&self) -> Value {
                Value::F32(*self as f32)
            }
        }
    )*};
}

macro_rules! integer_values {
    ($($t:ty),*) => {$(
        impl PropertyValue for $t {
            const KIND: PropKind = PropKind::F32;
            fn from_value(value: &Value) -> Option<Self> {
                let n = number(value)?.round();
                (n >= <$t>::MIN as f64 && n <= <$t>::MAX as f64).then_some(n as $t)
            }
            fn to_value(&self) -> Value {
                Value::F32(*self as f32)
            }
        }
    )*};
}

float_values!(f32, f64);
integer_values!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl PropertyValue for String {
    const KIND: PropKind = PropKind::String;
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(s.clone()),
            Value::Bool(b) => Some(b.to_string()),
            Value::F32(f) => Some(f.to_string()),
            Value::List(_) | Value::Object(_) => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::Str(self.clone())
    }
}

impl PropertyValue for Option<String> {
    const KIND: PropKind = PropKind::String;
    fn from_value(value: &Value) -> Option<Self> {
        String::from_value(value).map(|s| (!s.is_empty()).then_some(s))
    }
    fn to_value(&self) -> Value {
        Value::Str(self.clone().unwrap_or_default())
    }
}

/// A colour that follows the theme when it names a theme token (`#[property] accent_color:
/// Option<ColorValue>`, WinForms' `Color` property): the Properties window's colour editor (Theme,
/// Custom, Web, System tabs). `None` (an empty attribute) is the ambient colour.
impl PropertyValue for Option<crate::style::ColorValue> {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("color");
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => crate::style::parse_color(s).ok(),
            _ => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::Str(self.as_ref().map(crate::style::ColorValue::to_attribute).unwrap_or_default())
    }
}

/// A colour property that is always set (`#[property] accent_color: ColorValue`); an empty
/// attribute leaves the field as it is.
impl PropertyValue for crate::style::ColorValue {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("color");
    fn from_value(value: &Value) -> Option<Self> {
        <Option<crate::style::ColorValue>>::from_value(value).flatten()
    }
    fn to_value(&self) -> Value {
        Value::Str(self.to_attribute())
    }
}

/// A fixed drawing colour (`#[property] accent_color: Color`, `System.Drawing.Color`): a theme
/// token is read in the light theme — use [`crate::style::ColorValue`] to follow the theme. Written
/// back as `#RRGGBB(AA)`.
impl PropertyValue for kubuno_ui::graphics::Color {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("color");
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) if s.trim().is_empty() => Some(Self::default()),
            Value::Str(s) => crate::style::parse_color(s).ok().flatten().map(|c| c.to_color()),
            _ => None,
        }
    }
    fn to_value(&self) -> Value {
        let c = crate::style::Rgba::from_d2d(self.to_d2d());
        Value::Str(if self.is_transparent() && *self == Self::default() { String::new() } else { c.hex() })
    }
}

/// A list of strings (`#[property] countries: Vec<String>`, WinForms' `string[]` /
/// `StringCollection`): edited with the String Collection Editor, written one item per line
/// (`Countries="France&#10;Belgique"`); a binding may also hand it a list (each row's first text).
impl PropertyValue for Vec<String> {
    const KIND: PropKind = PropKind::String;
    const EDITOR: Option<&'static str> = Some("lines");
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(s.split('\n').map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect()),
            Value::List(rows) => Some(
                rows.iter()
                    .filter_map(|row| row.get("Text").or_else(|| row.fields().first().map(|(_, v)| v)).and_then(String::from_value))
                    .collect(),
            ),
            Value::Bool(_) | Value::F32(_) | Value::Object(_) => None,
        }
    }
    fn to_value(&self) -> Value {
        Value::Str(self.join("\n"))
    }
}

/// Raises a declared event (an `#[event]` field, EVT-7b): first to the field's Rust subscribers
/// (sender: the control), then queued for the `.kbview` handler its element names (delivered
/// when the element next paints, in the same frame when raised from a handler or an override).
/// What the generated `raise_<field>(args)` method calls.
pub fn raise_declared_event<A: EventArgs>(owner: &mut dyn Component, name: &'static str, event: &Event<A>, mut args: A) {
    {
        let owner_ref: &dyn Component = owner;
        let bounds = owner_ref.as_control().map(|c| c.control_core().bounds).unwrap_or_default();
        let focus_id = owner_ref.as_control().and_then(|c| c.control_core().focus_id);
        let sender = ElementRef { name: Some(owner_ref.display_name()), element: owner_ref.class_name(), id: "", bounds, focus_id, attributes: &[] };
        event.raise(&sender, &mut args);
    }
    owner.component_core_mut().queue_event(name, Box::new(args));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(bool::from_value(&Value::Str("true".into())), Some(true));
        assert_eq!(f32::from_value(&Value::Str(" 18.5 ".into())), Some(18.5));
        assert_eq!(u32::from_value(&Value::F32(4.6)), Some(5));
        assert_eq!(u8::from_value(&Value::F32(300.0)), None);
        assert_eq!(i32::from_value(&Value::Str("-3".into())), Some(-3));
        assert_eq!(String::from_value(&Value::F32(2.0)), Some("2".into()));
        assert_eq!(<Option<String>>::from_value(&Value::Str(String::new())), Some(None));
        assert_eq!(7u32.to_value(), Value::F32(7.0));
        assert!(matches!(<u32 as PropertyValue>::KIND, PropKind::F32));
    }
}
