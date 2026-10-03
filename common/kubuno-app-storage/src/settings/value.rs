//! The values a setting holds and their conversions: the JSON of the file back-end, the Registry's typed values, the
//! invariant text of `.kbsettings` defaults and of the Registry's `REG_SZ` floats.

use std::fmt;

/// The type of a setting (`Type=` of a `.kbsettings` entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingType {
    Bool,
    /// A 64-bit signed integer.
    Int,
    /// A 64-bit float.
    Float,
    String,
    /// An ordered list of strings (recent files, pinned items).
    StringList,
}

impl SettingType {
    /// Every type, in the order the designer's type drop-down shows them.
    pub const ALL: [SettingType; 5] = [SettingType::String, SettingType::Bool, SettingType::Int, SettingType::Float, SettingType::StringList];

    /// The `.kbsettings` spelling (with the .NET aliases a migrated `Settings.settings` uses).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "bool" | "boolean" | "system.boolean" => SettingType::Bool,
            "int" | "integer" | "int32" | "int64" | "long" | "system.int32" | "system.int64" => SettingType::Int,
            "float" | "double" | "single" | "system.double" | "system.single" => SettingType::Float,
            "string" | "system.string" => SettingType::String,
            "stringlist" | "stringcollection" | "system.collections.specialized.stringcollection" => SettingType::StringList,
            _ => return None,
        })
    }

    /// The canonical `.kbsettings` spelling.
    pub fn name(self) -> &'static str {
        match self {
            SettingType::Bool => "Bool",
            SettingType::Int => "Int",
            SettingType::Float => "Float",
            SettingType::String => "String",
            SettingType::StringList => "StringList",
        }
    }

    /// The Rust type a typed accessor returns.
    pub fn rust_type(self) -> &'static str {
        match self {
            SettingType::Bool => "bool",
            SettingType::Int => "i64",
            SettingType::Float => "f64",
            SettingType::String => "String",
            SettingType::StringList => "Vec<String>",
        }
    }

    /// The value of a setting with no default (`false`, `0`, `""`, `[]`).
    pub fn zero(self) -> SettingValue {
        match self {
            SettingType::Bool => SettingValue::Bool(false),
            SettingType::Int => SettingValue::Int(0),
            SettingType::Float => SettingValue::Float(0.0),
            SettingType::String => SettingValue::String(String::new()),
            SettingType::StringList => SettingValue::StringList(Vec::new()),
        }
    }
}

impl fmt::Display for SettingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A setting's value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    StringList(Vec<String>),
}

impl SettingValue {
    pub fn ty(&self) -> SettingType {
        match self {
            SettingValue::Bool(_) => SettingType::Bool,
            SettingValue::Int(_) => SettingType::Int,
            SettingValue::Float(_) => SettingType::Float,
            SettingValue::String(_) => SettingType::String,
            SettingValue::StringList(_) => SettingType::StringList,
        }
    }

    /// The value as `ty`, converting what converts without loss of meaning: an integer 0/1 or the text
    /// `true`/`false` to a bool (the Registry stores bools as `REG_DWORD`), an integer to a float, a number or a
    /// bool to its text, a text to a number (the Registry stores floats as `REG_SZ`), a text to a one-item list.
    /// `None` when it does not convert (the setting then falls back to its default).
    pub fn coerce(&self, ty: SettingType) -> Option<SettingValue> {
        use SettingValue as V;
        Some(match (self, ty) {
            (v, t) if v.ty() == t => v.clone(),
            (V::Int(0), SettingType::Bool) => V::Bool(false),
            (V::Int(1), SettingType::Bool) => V::Bool(true),
            (V::String(s), SettingType::Bool) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" => V::Bool(true),
                "false" | "0" => V::Bool(false),
                _ => return None,
            },
            (V::Int(i), SettingType::Float) => V::Float(*i as f64),
            (V::Float(f), SettingType::Int) if f.fract() == 0.0 && f.is_finite() && f.abs() < 9.0e15 => V::Int(*f as i64),
            (V::String(s), SettingType::Int) => V::Int(s.trim().parse().ok()?),
            (V::String(s), SettingType::Float) => V::Float(parse_float(s)?),
            (V::Bool(b), SettingType::String) => V::String(b.to_string()),
            (V::Int(i), SettingType::String) => V::String(i.to_string()),
            (V::Float(f), SettingType::String) => V::String(format_float(*f)),
            (V::String(s), SettingType::StringList) => V::StringList(vec![s.clone()]),
            _ => return None,
        })
    }

    /// Parses the invariant text of a `ty` value (a `.kbsettings` `Default=`): `true`/`false`, `42`, `1.5`, any
    /// text; a list is one item per line.
    pub fn parse(text: &str, ty: SettingType) -> Option<SettingValue> {
        match ty {
            SettingType::StringList => Some(SettingValue::StringList(if text.is_empty() { Vec::new() } else { text.lines().map(str::to_string).collect() })),
            SettingType::String => Some(SettingValue::String(text.to_string())),
            _ => SettingValue::String(text.to_string()).coerce(ty),
        }
    }

    /// The invariant text of the value (what [`SettingValue::parse`] reads back; a list is one item per line).
    pub fn to_text(&self) -> String {
        match self {
            SettingValue::Bool(b) => b.to_string(),
            SettingValue::Int(i) => i.to_string(),
            SettingValue::Float(f) => format_float(*f),
            SettingValue::String(s) => s.clone(),
            SettingValue::StringList(l) => l.join("\n"),
        }
    }

    /// The value as JSON (the file back-end).
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            SettingValue::Bool(b) => serde_json::Value::Bool(*b),
            SettingValue::Int(i) => serde_json::Value::from(*i),
            SettingValue::Float(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
            SettingValue::String(s) => serde_json::Value::String(s.clone()),
            SettingValue::StringList(l) => serde_json::Value::Array(l.iter().cloned().map(serde_json::Value::String).collect()),
        }
    }

    /// The natural value of a JSON value; `None` for what no setting holds (objects, `null`, mixed arrays). Such
    /// values (written by a newer version of the app) are kept untouched in the file.
    pub fn from_json(json: &serde_json::Value) -> Option<SettingValue> {
        Some(match json {
            serde_json::Value::Bool(b) => SettingValue::Bool(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => SettingValue::Int(i),
                None => SettingValue::Float(n.as_f64()?),
            },
            serde_json::Value::String(s) => SettingValue::String(s.clone()),
            serde_json::Value::Array(items) => SettingValue::StringList(items.iter().map(|i| i.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()?),
            _ => return None,
        })
    }

    /// The bytes the value takes (the limits of `docs/STORAGE-COMPONENTS.md` §6.3).
    pub fn byte_len(&self) -> usize {
        match self {
            SettingValue::Bool(_) => 1,
            SettingValue::Int(_) | SettingValue::Float(_) => 8,
            SettingValue::String(s) => s.len(),
            SettingValue::StringList(l) => l.iter().map(|s| s.len() + 1).sum(),
        }
    }
}

/// Floats round-trip through their shortest text (`0.1` stays `0.1`).
fn format_float(f: f64) -> String {
    let s = f.to_string();
    if s.contains(['.', 'e', 'E']) || !f.is_finite() {
        s
    } else {
        format!("{s}.0")
    }
}

/// An invariant float: `.` is the only decimal separator (a `,` is refused rather than guessed).
fn parse_float(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

impl From<bool> for SettingValue {
    fn from(v: bool) -> Self {
        SettingValue::Bool(v)
    }
}
impl From<i64> for SettingValue {
    fn from(v: i64) -> Self {
        SettingValue::Int(v)
    }
}
impl From<i32> for SettingValue {
    fn from(v: i32) -> Self {
        SettingValue::Int(v.into())
    }
}
impl From<u32> for SettingValue {
    fn from(v: u32) -> Self {
        SettingValue::Int(v.into())
    }
}
impl From<f64> for SettingValue {
    fn from(v: f64) -> Self {
        SettingValue::Float(v)
    }
}
impl From<f32> for SettingValue {
    fn from(v: f32) -> Self {
        SettingValue::Float(v.into())
    }
}
impl From<&str> for SettingValue {
    fn from(v: &str) -> Self {
        SettingValue::String(v.to_string())
    }
}
impl From<String> for SettingValue {
    fn from(v: String) -> Self {
        SettingValue::String(v)
    }
}
impl From<&String> for SettingValue {
    fn from(v: &String) -> Self {
        SettingValue::String(v.clone())
    }
}
impl From<Vec<String>> for SettingValue {
    fn from(v: Vec<String>) -> Self {
        SettingValue::StringList(v)
    }
}
impl From<&[&str]> for SettingValue {
    fn from(v: &[&str]) -> Self {
        SettingValue::StringList(v.iter().map(|s| s.to_string()).collect())
    }
}

/// A Rust type a setting can be read as (`settings.get_as::<bool>("ShowHidden")`).
pub trait FromSetting: Sized {
    /// The setting type it reads.
    const TYPE: SettingType;
    fn from_setting(value: &SettingValue) -> Option<Self>;
}

macro_rules! from_setting {
    ($t:ty, $st:ident, $v:ident => $e:expr) => {
        impl FromSetting for $t {
            const TYPE: SettingType = SettingType::$st;
            fn from_setting(value: &SettingValue) -> Option<Self> {
                match value.coerce(SettingType::$st)? {
                    SettingValue::$st($v) => $e,
                    _ => None,
                }
            }
        }
    };
}

from_setting!(bool, Bool, v => Some(v));
from_setting!(i64, Int, v => Some(v));
from_setting!(i32, Int, v => i32::try_from(v).ok());
from_setting!(u32, Int, v => u32::try_from(v).ok());
from_setting!(f64, Float, v => Some(v));
from_setting!(f32, Float, v => Some(v as f32));
from_setting!(String, String, v => Some(v));
from_setting!(Vec<String>, StringList, v => Some(v));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coercions_follow_the_back_ends() {
        assert_eq!(SettingValue::Int(1).coerce(SettingType::Bool), Some(SettingValue::Bool(true)));
        assert_eq!(SettingValue::Int(2).coerce(SettingType::Bool), None);
        assert_eq!(SettingValue::String("1.25".into()).coerce(SettingType::Float), Some(SettingValue::Float(1.25)));
        assert_eq!(SettingValue::String("1,25".into()).coerce(SettingType::Float), None);
        assert_eq!(SettingValue::Float(3.0).coerce(SettingType::Int), Some(SettingValue::Int(3)));
        assert_eq!(SettingValue::Float(3.5).coerce(SettingType::Int), None);
        assert_eq!(SettingValue::Bool(true).coerce(SettingType::StringList), None);
    }

    #[test]
    fn text_and_json_round_trip() {
        for v in [
            SettingValue::Bool(true),
            SettingValue::Int(-42),
            SettingValue::Float(0.1),
            SettingValue::Float(2.0),
            SettingValue::String("a \"b\"\nc".into()),
            SettingValue::StringList(vec!["x".into(), "y z".into()]),
        ] {
            assert_eq!(SettingValue::from_json(&v.to_json()).and_then(|j| j.coerce(v.ty())), Some(v.clone()), "{v:?} through JSON");
            if !matches!(&v, SettingValue::String(s) if s.contains('\n')) {
                assert_eq!(SettingValue::parse(&v.to_text(), v.ty()), Some(v.clone()), "{v:?} through text");
            }
        }
        assert_eq!(SettingValue::parse("", SettingType::StringList), Some(SettingValue::StringList(vec![])));
        assert_eq!(SettingValue::from_json(&serde_json::json!({"a": 1})), None);
    }

    #[test]
    fn typed_reads() {
        assert_eq!(bool::from_setting(&SettingValue::Int(0)), Some(false));
        assert_eq!(u32::from_setting(&SettingValue::Int(-1)), None);
        assert_eq!(String::from_setting(&SettingValue::Int(7)).as_deref(), Some("7"));
        assert_eq!(<Vec<String>>::from_setting(&SettingValue::String("a".into())), Some(vec!["a".to_string()]));
    }
}
