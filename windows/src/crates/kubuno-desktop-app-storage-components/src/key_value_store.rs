//! `<KeyValueStore>`: small untyped values of the app (`kubuno_desktop_app_storage::KeyValueStore`): a last-opened folder,
//! a dismissed tip, a cached answer with a time to live.
//!
//! ```xml
//! <KeyValueStore x:Name="state" Store="state"/>
//! <TextField Text="{Binding lastSearch, Source=state, Mode=TwoWay}"/>
//! ```
//!
//! Paths are keys (a key with dots cannot be bound: use code). A two-way binding writes at once. Code:
//! `self.state.set("lastFolder", path)`, `get_string`, `remove`. In the designer the store is in memory.

use std::time::Duration;

use kubuno_desktop_app_storage::{default_app_id, AppId, KeyValueStore as Engine, Persistence, StorageError};
use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::format::ValueKind;
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::scope::{BindingProvider, ComponentScope};

/// Where the values live (`Persistence=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum KeyValuePersistence {
    /// In a file of the app's data folder.
    #[default]
    Persistent,
    /// In memory, for the life of the process.
    Session,
}

#[derive(Debug, Clone, PartialEq)]
struct Opened {
    store: String,
    app_id: String,
    persistence: KeyValuePersistence,
    account_scoped: bool,
    account_generation: u64,
    design: bool,
}

/// Small untyped values of the app, in a file of its data folder (or in memory). Bound by key: {Binding lastFolder, Source=state}.
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "braces", category = "Storage")]
#[default_property("Store")]
pub struct KeyValueStore {
    base: ComponentCore,
    /// The store's name: one file per store (state for <data>/<app>/kv/state.json).
    #[property]
    #[category("Storage")]
    #[default_value("state")]
    pub store: String,
    /// The app the values belong to (its id); empty for the application's own.
    #[property]
    #[category("Storage")]
    pub app_id: String,
    /// Persistent (a file of the app's data folder) or Session (memory, for the life of the process).
    #[property]
    #[category("Storage")]
    #[default_value("Persistent")]
    pub persistence: KeyValuePersistence,
    /// Whether the values belong to the signed-in account (each account its own).
    #[property]
    #[category("Storage")]
    #[default_value(false)]
    pub account_scoped: bool,
    engine: Option<(Opened, Engine)>,
    last_error: Option<String>,
}

impl Default for KeyValueStore {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            store: "state".into(),
            app_id: String::new(),
            persistence: KeyValuePersistence::Persistent,
            account_scoped: false,
            engine: None,
            last_error: None,
        }
    }
}

impl std::fmt::Debug for KeyValueStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyValueStore").field("store", &self.store).field("persistence", &self.persistence).finish()
    }
}

impl KeyValueStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(&self) -> Opened {
        Opened {
            store: self.store.trim().to_string(),
            app_id: self.app_id.trim().to_string(),
            persistence: self.persistence,
            account_scoped: self.account_scoped,
            account_generation: kubuno_desktop_app_storage::account::account_generation(),
            design: crate::designing(self.design_mode()),
        }
    }

    /// The store behind the component (opened on first use, reopened when a property or the account changed).
    pub fn engine(&mut self) -> Result<Engine, StorageError> {
        let key = self.key();
        if let Some((k, e)) = &self.engine {
            if *k == key {
                return Ok(e.clone());
            }
        }
        let name = if key.store.is_empty() { "state" } else { key.store.as_str() };
        let opened = if key.design {
            Ok(Engine::in_memory(name))
        } else {
            let app = if key.app_id.is_empty() { default_app_id() } else { AppId::new(&key.app_id)? };
            let persistence = match key.persistence {
                KeyValuePersistence::Persistent => Persistence::Persistent,
                KeyValuePersistence::Session => Persistence::Session,
            };
            match (key.account_scoped, kubuno_desktop_app_storage::current_account()) {
                (true, None) => Ok(Engine::in_memory(name)),
                (true, Some(a)) => Engine::open(&app, name, persistence, Some(&a)),
                (false, _) => Engine::open(&app, name, persistence, None),
            }
        };
        match opened {
            Ok(e) => {
                self.last_error = None;
                self.engine = Some((key, e.clone()));
                Ok(e)
            }
            Err(e) => {
                tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), "the key/value store cannot be opened: {e}");
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    pub fn get(&mut self, key: &str) -> Option<serde_json::Value> {
        self.engine().ok()?.get(key)
    }

    pub fn get_string(&mut self, key: &str) -> Option<String> {
        self.engine().ok()?.get_string(key)
    }

    /// Sets `key` (written at once), optionally expiring after `ttl`.
    pub fn set(&mut self, key: &str, value: impl Into<serde_json::Value>, ttl: Option<Duration>) -> Result<(), StorageError> {
        self.engine()?.set(key, value, ttl)
    }

    pub fn remove(&mut self, key: &str) -> Result<bool, StorageError> {
        self.engine()?.remove(key)
    }

    pub fn keys(&mut self) -> Vec<String> {
        self.engine().map(|e| e.keys()).unwrap_or_default()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Component for KeyValueStore {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// Paths are keys.
impl BindingProvider for KeyValueStore {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        let (_, engine) = self.engine.as_ref()?;
        let v = match engine.get(path)? {
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => Value::F32(n.as_f64().unwrap_or(0.0) as f32),
            serde_json::Value::String(s) => Value::Str(s),
            other => Value::Str(other.to_string()),
        };
        kubuno_desktop_views::format::to_target(v, want, format)
    }

    fn binding_set(&mut self, path: &str, value: Value, format: &BindingFormat, _scope: &ComponentScope) -> bool {
        let json = match kubuno_desktop_views::format::from_target(value, format) {
            Value::Bool(b) => serde_json::Value::Bool(b),
            Value::F32(f) => serde_json::Number::from_f64(f64::from(f)).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
            Value::Str(s) => serde_json::Value::String(s),
            _ => return true,
        };
        if let Err(e) = self.set(path, json, None) {
            tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), key = path, "the value was not stored: {e}");
            self.last_error = Some(e.to_string());
        }
        true
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        let before = self.engine.as_ref().map(|(k, _)| k.clone());
        let _ = self.engine();
        before != self.engine.as_ref().map(|(k, _)| k.clone())
    }
}
