//! An app's existing settings **struct** stored through [`Settings`] (`docs/STORAGE-COMPONENTS.md` lot ST-2): each
//! top-level field of the struct is one setting of an open set, so an app with a large `#[derive(Serialize,
//! Deserialize)]` struct (drive's ~110 options) moves onto the storage engine — its files, Registry back-end, sandbox,
//! change notifications and migration — without writing a `.kbsettings` entry per field.
//!
//! Booleans, numbers, strings (serde's unit enum variants included) and lists of strings are stored as such; any
//! other field (a nested struct, an `Option` that is `None`, a list of non-strings) is stored as its JSON text and read
//! back by the shape of the struct's default. A field missing from the store reads as the default's.

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::store::Settings;
use super::value::SettingValue;
use crate::StorageError;

/// The settings of `value`'s top-level fields (see the module doc).
pub fn to_values<T: Serialize>(value: &T) -> Result<Vec<(String, SettingValue)>, StorageError> {
    let json = serde_json::to_value(value).map_err(|e| StorageError::Backend { backend: "serde", message: e.to_string() })?;
    let serde_json::Value::Object(map) = json else {
        return Err(StorageError::Backend { backend: "serde", message: "the settings type is not a struct".into() });
    };
    Ok(map.into_iter().map(|(k, v)| (k, natural_or_text(&v))).collect())
}

fn natural_or_text(v: &serde_json::Value) -> SettingValue {
    match v {
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => SettingValue::from_json(v).unwrap_or(SettingValue::String(v.to_string())),
        serde_json::Value::Array(items) if items.iter().all(serde_json::Value::is_string) => SettingValue::from_json(v).unwrap_or(SettingValue::String(v.to_string())),
        other => SettingValue::String(other.to_string()),
    }
}

/// Reads a `T` from `settings`, field by field over `T::default()` (a field that does not read keeps its default;
/// a struct that does not deserialize at all is the default, and the error is logged).
pub fn load<T: Serialize + DeserializeOwned + Default>(settings: &Settings) -> T {
    let default = T::default();
    let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&default) else { return default };
    for (name, slot) in map.iter_mut() {
        let Some(stored) = settings.get(name) else { continue };
        let read = match (&stored, &*slot) {
            // A JSON text where the default is not a string: what `to_values` wrote for a complex field.
            (SettingValue::String(text), d) if !d.is_string() => serde_json::from_str::<serde_json::Value>(text).ok(),
            (v, serde_json::Value::Number(n)) if n.is_f64() => v.coerce(crate::SettingType::Float).map(|v| v.to_json()),
            (v, serde_json::Value::Number(_)) => v.coerce(crate::SettingType::Int).map(|v| v.to_json()),
            (v, _) => Some(v.to_json()),
        };
        if let Some(v) = read {
            *slot = v;
        }
    }
    match serde_json::from_value::<T>(serde_json::Value::Object(map)) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "kubuno_app_storage", set = %settings.schema().set, "the stored settings do not fit the settings type, its defaults are used: {e}");
            default
        }
    }
}

/// Stores `value`: every field that differs from what the set holds, then saves. Returns whether anything changed.
pub fn save<T: Serialize>(settings: &Settings, value: &T) -> Result<bool, StorageError> {
    let mut changed = false;
    for (name, v) in to_values(value)? {
        if settings.get(&name).as_ref() != Some(&v) {
            settings.set(&name, v)?;
            changed = true;
        }
    }
    settings.save()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MemoryBackend;
    use crate::settings::{SettingsOptions, SettingsSchema};
    use crate::AppId;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct Prefs {
        theme: Mode,
        show_hidden: bool,
        zoom: f64,
        count: u32,
        name: String,
        recent: Vec<String>,
        columns: Vec<Column>,
        window: Option<(i32, i32)>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Mode {
        System,
        Dark,
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Column {
        id: String,
        width: f32,
    }

    impl Default for Prefs {
        fn default() -> Self {
            Self { theme: Mode::System, show_hidden: false, zoom: 1.0, count: 3, name: String::new(), recent: Vec::new(), columns: Vec::new(), window: None }
        }
    }

    #[test]
    fn a_struct_round_trips_field_by_field() {
        let app = AppId::new("bridge-test").expect("id");
        let s = Settings::with_backend(&app, SettingsSchema::open_local("prefs"), Box::new(MemoryBackend::new()), SettingsOptions::default()).expect("open");
        assert_eq!(load::<Prefs>(&s), Prefs::default(), "an empty store reads as the default");
        let p = Prefs {
            theme: Mode::Dark,
            show_hidden: true,
            zoom: 1.25,
            count: 7,
            name: "x \"y\"".into(),
            recent: vec!["a".into(), "b".into()],
            columns: vec![Column { id: "name".into(), width: 120.0 }],
            window: Some((10, 20)),
        };
        assert!(save(&s, &p).expect("save"));
        assert!(!save(&s, &p).expect("again"), "nothing changed");
        assert_eq!(load::<Prefs>(&s), p);
        assert_eq!(s.get("theme"), Some(SettingValue::String("Dark".into())), "an enum is stored as its name");
        assert!(matches!(s.get("columns"), Some(SettingValue::String(_))), "a list of structs as JSON text");
        assert_eq!(s.location(crate::Layer::UserLocal), "memory (user (local))");
        // A value of the wrong shape (edited by hand) keeps that field's default only.
        s.set("count", "many").expect("set");
        let read = load::<Prefs>(&s);
        assert_eq!((read.count, read.zoom), (3, 1.25));
    }
}
