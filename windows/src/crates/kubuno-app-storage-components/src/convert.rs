//! Settings values ⇄ binding values.

use kubuno_app_storage::{SettingType, SettingValue};
use kubuno_views::binding::{Row, Value};

/// The field of the rows a `StringList` gives an `ItemsSource`.
pub(crate) const ITEM_FIELD: &str = "Value";

/// What a binding reads for a setting's value: numbers as numbers, a list as rows (`ItemsSource`).
pub(crate) fn to_view(v: &SettingValue) -> Value {
    match v {
        SettingValue::Bool(b) => Value::Bool(*b),
        SettingValue::Int(i) => Value::F32(*i as f32),
        SettingValue::Float(f) => Value::F32(*f as f32),
        SettingValue::String(s) => Value::Str(s.clone()),
        SettingValue::StringList(l) => Value::List(l.iter().map(|s| Row::new().with(ITEM_FIELD, Value::Str(s.clone()))).collect::<Vec<_>>().into()),
    }
}

/// What a two-way binding writes, as a value of `ty` (`None`: the setting is open, keep the natural type).
pub(crate) fn from_view(v: Value, ty: Option<SettingType>) -> Option<SettingValue> {
    let natural = match v {
        Value::Bool(b) => SettingValue::Bool(b),
        Value::F32(f) => match ty {
            Some(SettingType::Int) if f.is_finite() => SettingValue::Int(f.round() as i64),
            _ => SettingValue::Float(f64::from(f)),
        },
        Value::Str(s) => match ty {
            Some(t) => return SettingValue::parse(&s, t),
            None => SettingValue::String(s),
        },
        Value::List(rows) => SettingValue::StringList(rows.iter().map(|r| r.fields().first().map(|(_, v)| text_of(v)).unwrap_or_default()).collect()),
        Value::Object(_) => return None,
    };
    match ty {
        Some(t) => natural.coerce(t),
        None => Some(natural),
    }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::F32(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cross_the_binding() {
        assert_eq!(from_view(Value::F32(4.6), Some(SettingType::Int)), Some(SettingValue::Int(5)));
        assert_eq!(from_view(Value::Str("12".into()), Some(SettingType::Int)), Some(SettingValue::Int(12)));
        assert_eq!(from_view(Value::Str("x".into()), Some(SettingType::Int)), None);
        assert_eq!(from_view(Value::Bool(true), Some(SettingType::String)), Some(SettingValue::String("true".into())));
        let list = SettingValue::StringList(vec!["a".into(), "b".into()]);
        assert_eq!(from_view(to_view(&list), Some(SettingType::StringList)), Some(list));
        assert_eq!(from_view(Value::F32(1.5), None), Some(SettingValue::Float(1.5)));
    }
}
