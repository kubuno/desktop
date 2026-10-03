//! `<SecretStore>`: the app's secrets (an API key, a mail account's password) in the OS credential store, scoped to
//! the app (`kubuno_app_storage::AppSecrets`).
//!
//! ```xml
//! <SecretStore x:Name="secrets"/>
//! <Label Visible="{Binding ApiKey.Exists, Source=secrets}" Text="Clé enregistrée"/>
//! ```
//!
//! ```ignore
//! self.secrets.set_str("ApiKey", &self.key_field.get_text())?;   // code only: a secret is never bound
//! ```
//!
//! **Bindings never see a value**: the paths are `Available` (the store can be used) and `<Name>.Exists`, both
//! read-only. A secret goes in and out through code, where it is a `Secret` (zeroed on drop, redacted in logs).

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use kubuno_app_storage::{default_app_id, AppId, AppSecrets, Secret, StorageError};
use kubuno_views::binding::{BindingFormat, Value};
use kubuno_views::format::ValueKind;
use kubuno_views::prelude::*;
use kubuno_views::scope::{BindingProvider, ComponentScope};

/// How long an `Exists` answer is kept before the store is asked again.
const EXISTS_TTL: Duration = Duration::from_secs(2);

/// Where the secrets are kept (`Backend=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SecretBackend {
    /// The OS credential store: Windows Credential Manager (never roams), macOS Keychain, Secret Service on Linux.
    #[default]
    Os,
    /// In memory, for the life of the process (samples, tests).
    Memory,
}

/// The app's secrets (API keys, passwords) in the system's credential store. Set and read them from code: bindings only tell whether one exists ({Binding ApiKey.Exists, Source=secrets}).
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "key-round", category = "Storage")]
#[default_property("AppId")]
pub struct SecretStore {
    base: ComponentCore,
    /// The app the secrets belong to (its id, kubuno-notes); empty for the application's own.
    #[property]
    #[category("Storage")]
    pub app_id: String,
    /// Where the secrets are kept: Os (the system's credential store) or Memory.
    #[property]
    #[category("Storage")]
    #[default_value("Os")]
    pub backend: SecretBackend,
    secrets: Option<((String, SecretBackend, bool), AppSecrets)>,
    exists: RefCell<HashMap<String, (bool, Instant)>>,
    last_error: Option<String>,
}

impl Default for SecretStore {
    fn default() -> Self {
        Self { base: ComponentCore::default(), app_id: String::new(), backend: SecretBackend::Os, secrets: None, exists: RefCell::new(HashMap::new()), last_error: None }
    }
}

impl std::fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretStore").field("app_id", &self.app_id).field("backend", &self.backend).finish()
    }
}

/// The in-memory stores of the process, one per app, so that every `<SecretStore Backend="Memory">` of an app and
/// the designer share them.
fn memory_store(app: &AppId) -> AppSecrets {
    static STORES: std::sync::OnceLock<std::sync::Mutex<HashMap<AppId, AppSecrets>>> = std::sync::OnceLock::new();
    let map = STORES.get_or_init(Default::default);
    let mut m = map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    m.entry(app.clone()).or_insert_with(|| AppSecrets::in_memory(app)).clone()
}

impl SecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The app's secrets (opened on first use, reopened when `AppId` or `Backend` changed).
    pub fn secrets(&mut self) -> Result<AppSecrets, StorageError> {
        let design = crate::designing(self.design_mode());
        let key = (self.app_id.trim().to_string(), self.backend, design);
        if let Some((k, s)) = &self.secrets {
            if *k == key {
                return Ok(s.clone());
            }
        }
        let app = if key.0.is_empty() { default_app_id() } else { AppId::new(&key.0)? };
        let opened = if design || self.backend == SecretBackend::Memory { memory_store(&app) } else { Self::os(&app)? };
        self.exists.borrow_mut().clear();
        self.secrets = Some((key, opened.clone()));
        Ok(opened)
    }

    fn os(app: &AppId) -> Result<AppSecrets, StorageError> {
        AppSecrets::open(app)
    }

    /// The secret `name`, `None` when there is none.
    pub fn get(&mut self, name: &str) -> Result<Option<Secret>, StorageError> {
        self.secrets()?.get(name)
    }

    /// Creates or replaces the secret `name`.
    pub fn set(&mut self, name: &str, value: &Secret) -> Result<(), StorageError> {
        let r = self.secrets()?.set(name, value);
        self.exists.borrow_mut().remove(name);
        r
    }

    /// Creates or replaces a text secret.
    pub fn set_str(&mut self, name: &str, value: &str) -> Result<(), StorageError> {
        self.set(name, &Secret::from_str_value(value))
    }

    /// Removes the secret `name`; `Ok(false)` when there was none.
    pub fn delete(&mut self, name: &str) -> Result<bool, StorageError> {
        let r = self.secrets()?.delete(name);
        self.exists.borrow_mut().remove(name);
        r
    }

    pub fn contains(&mut self, name: &str) -> Result<bool, StorageError> {
        self.secrets()?.contains(name)
    }

    /// Why the store could not be used, if it could not (the binding `Available` is then false).
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn cached_exists(&self, name: &str) -> Option<bool> {
        if let Some((v, at)) = self.exists.borrow().get(name) {
            if at.elapsed() < EXISTS_TTL {
                return Some(*v);
            }
        }
        let (_, secrets) = self.secrets.as_ref()?;
        let v = secrets.contains(name).ok()?;
        self.exists.borrow_mut().insert(name.to_string(), (v, Instant::now()));
        Some(v)
    }
}

impl Component for SecretStore {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// `Available` and `<Name>.Exists` only (see the module doc).
impl BindingProvider for SecretStore {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        let v = match path.rsplit_once('.') {
            None if path == "Available" => self.secrets.is_some() && self.last_error.is_none(),
            Some((name, "Exists")) => self.cached_exists(name)?,
            _ => return None,
        };
        kubuno_views::format::to_target(Value::Bool(v), want, format)
    }

    fn binding_set(&mut self, path: &str, _value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
        tracing::warn!(target: "kubuno_app_storage", component = %self.display_name(), path, "a secret store is read-only for bindings: set secrets from code");
        true
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        if self.secrets.is_none() {
            match self.secrets() {
                Ok(_) => self.last_error = None,
                Err(e) => {
                    if self.last_error.is_none() {
                        tracing::warn!(target: "kubuno_app_storage", component = %self.display_name(), "the secret store cannot be opened: {e}");
                    }
                    self.last_error = Some(e.to_string());
                }
            }
        }
        false
    }
}
