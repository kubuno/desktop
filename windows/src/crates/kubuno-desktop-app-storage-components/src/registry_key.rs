//! `<RegistryKey>` (Windows only): one key of the Windows Registry in a view, its values bindable by name
//! (.NET's `Microsoft.Win32.RegistryKey` as a component).
//!
//! ```xml
//! <RegistryKey x:Name="advanced" Hive="CurrentUser" Path="Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"/>
//! <Label Text="{Binding Hidden, Source=advanced}"/>
//! <Label Visible="{Binding Source=advanced}" Text="…"/>   <!-- the path alone: whether the key exists -->
//! ```
//!
//! - `Hive`: `CurrentUser` (default), `LocalMachine`, `ClassesRoot`, `Users`, `CurrentConfig`; `View`: `Default`,
//!   `Registry32`, `Registry64` (WOW64); `Writable`: bindings and code may write values (the key is created on the
//!   first write). Read-only by default: a view that only shows a value cannot change the system by accident.
//! - A path below the component is a value name; a number reads as a number, a list as its lines. Values
//!   written through a two-way binding keep the type the value already has (`REG_DWORD` stays a `REG_DWORD`),
//!   `REG_SZ` for a new one.
//! - The key is polled every two seconds: `ValueChanged` (external) and a repaint when something changed it.
//! - Every access goes through `RegistryRoot::current()`: in a sandboxed profile (`KUBUNO_SANDBOX_DIR`) the key is
//!   the sandbox's copy below `HKCU\Software\Kubuno\Sandbox\<tag>`. In the designer the Registry is not read.
//! - Elsewhere than Windows the component exists (a portable view still loads) but reads nothing and every call
//!   returns `StorageError::Unsupported`; the language server warns when a project targeting another OS uses it.

use std::time::{Duration, Instant};

#[cfg(windows)]
use kubuno_desktop_app_storage::registry::{Access, Hive, RegValue, RegistryRoot, RegistryView};
use kubuno_desktop_app_storage::StorageError;
use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::events::Event;
use kubuno_desktop_views::format::ValueKind;
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::scope::{BindingProvider, ComponentScope};

use crate::args::{emit, RegistryValueChangedEventArgs};

/// How often the key is checked for changes made outside.
const POLL_EVERY: Duration = Duration::from_secs(2);

/// The root key (`Hive=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RegistryHive {
    /// HKEY_CURRENT_USER.
    #[default]
    CurrentUser,
    /// HKEY_LOCAL_MACHINE: writing it needs administrator rights.
    LocalMachine,
    /// HKEY_CLASSES_ROOT.
    ClassesRoot,
    /// HKEY_USERS.
    Users,
    /// HKEY_CURRENT_CONFIG.
    CurrentConfig,
}

/// The WOW64 view (`View=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RegistryKeyView {
    #[default]
    Default,
    Registry32,
    Registry64,
}

#[cfg(windows)]
impl RegistryHive {
    fn hive(self) -> Hive {
        match self {
            RegistryHive::CurrentUser => Hive::CurrentUser,
            RegistryHive::LocalMachine => Hive::LocalMachine,
            RegistryHive::ClassesRoot => Hive::ClassesRoot,
            RegistryHive::Users => Hive::Users,
            RegistryHive::CurrentConfig => Hive::CurrentConfig,
        }
    }
}

#[cfg(windows)]
impl RegistryKeyView {
    fn view(self) -> RegistryView {
        match self {
            RegistryKeyView::Default => RegistryView::Default,
            RegistryKeyView::Registry32 => RegistryView::Registry32,
            RegistryKeyView::Registry64 => RegistryView::Registry64,
        }
    }
}

/// What a value is, for a binding.
#[derive(Debug, Clone, PartialEq)]
enum Cached {
    Text(String),
    Number(f64),
    Lines(Vec<String>),
}

/// One key of the Windows Registry: its values bindable by name ({Binding Name, Source=key}), read-only unless Writable. Windows only.
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "folder-key", category = "Storage")]
#[default_event("ValueChanged")]
#[default_property("Path")]
pub struct RegistryKey {
    base: ComponentCore,
    /// The root key: CurrentUser (HKCU), LocalMachine (HKLM), ClassesRoot, Users or CurrentConfig.
    #[property]
    #[category("Storage")]
    #[default_value("CurrentUser")]
    pub hive: RegistryHive,
    /// The key's path below its root (Software\Contoso\App). The … button browses the Registry.
    #[property]
    #[category("Storage")]
    #[editor("registry-key")]
    pub path: String,
    /// The WOW64 view: Default, Registry32 (WOW6432Node) or Registry64.
    #[property]
    #[category("Storage")]
    #[default_value("Default")]
    pub view: RegistryKeyView,
    /// Whether bindings and code may write values (the key is created on the first write).
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub writable: bool,
    /// Occurs when a value was written through a binding, or the key changed outside.
    #[event]
    #[category("Property Changed")]
    pub value_changed: Event<RegistryValueChangedEventArgs>,
    /// The values read at the last poll (bindings read from here, never the Registry itself, each frame).
    cache: Option<(String, Option<u64>, std::collections::HashMap<String, Cached>)>,
    last_poll: Option<Instant>,
    last_error: Option<String>,
}

impl Default for RegistryKey {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            hive: RegistryHive::CurrentUser,
            path: String::new(),
            view: RegistryKeyView::Default,
            writable: false,
            value_changed: Event::default(),
            cache: None,
            last_poll: None,
            last_error: None,
        }
    }
}

impl std::fmt::Debug for RegistryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryKey").field("hive", &self.hive).field("path", &self.path).field("view", &self.view).field("writable", &self.writable).finish()
    }
}

impl RegistryKey {
    pub fn new() -> Self {
        Self::default()
    }

    /// A key of `hive` at `path`.
    pub fn at(hive: RegistryHive, path: &str) -> Self {
        Self { hive, path: path.to_string(), ..Self::default() }
    }

    /// Why the key could not be read or written last time, if it could not.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// `HKCU\Software\…` (the real location: the sandbox's copy in a sandbox).
    pub fn full_name(&self) -> String {
        #[cfg(windows)]
        {
            RegistryRoot::current().display(self.hive.hive(), self.path.trim())
        }
        #[cfg(not(windows))]
        {
            self.path.clone()
        }
    }

    #[cfg(windows)]
    fn open(&self, access: Access) -> Result<Option<kubuno_desktop_app_storage::registry::RegistryKey>, StorageError> {
        RegistryRoot::current().open(self.hive.hive(), self.path.trim(), self.view.view(), access)
    }

    /// Whether the key exists.
    pub fn exists(&self) -> Result<bool, StorageError> {
        #[cfg(windows)]
        {
            Ok(self.open(Access::Read)?.is_some())
        }
        #[cfg(not(windows))]
        {
            Err(StorageError::Unsupported("the Windows Registry"))
        }
    }

    /// The value `name` (`""`: the key's default value); `None` when the key or the value does not exist.
    #[cfg(windows)]
    pub fn get_value(&self, name: &str) -> Result<Option<RegValue>, StorageError> {
        match self.open(Access::Read)? {
            Some(k) => k.get_value(name),
            None => Ok(None),
        }
    }

    /// The value `name` as text (numbers in decimal, lists one item per line).
    pub fn get_string(&self, name: &str) -> Result<Option<String>, StorageError> {
        #[cfg(windows)]
        {
            Ok(self.get_value(name)?.map(|v| v.to_text()))
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Err(StorageError::Unsupported("the Windows Registry"))
        }
    }

    /// Writes the value `name` (the key is created when missing). Refused unless `Writable`.
    #[cfg(windows)]
    pub fn set_value(&mut self, name: &str, value: &RegValue) -> Result<(), StorageError> {
        if !self.writable {
            return Err(StorageError::ReadOnly(format!("{} (Writable is false)", self.full_name())));
        }
        RegistryRoot::current().create(self.hive.hive(), self.path.trim(), self.view.view())?.set_value(name, value)?;
        self.cache = None;
        Ok(())
    }

    /// Removes the value `name`; `Ok(false)` when there was none. Refused unless `Writable`.
    #[cfg(windows)]
    pub fn delete_value(&mut self, name: &str) -> Result<bool, StorageError> {
        if !self.writable {
            return Err(StorageError::ReadOnly(format!("{} (Writable is false)", self.full_name())));
        }
        let r = match self.open(Access::ReadWrite)? {
            Some(k) => k.delete_value(name),
            None => Ok(false),
        };
        self.cache = None;
        r
    }

    /// The names of the values (empty when the key does not exist).
    #[cfg(windows)]
    pub fn value_names(&self) -> Result<Vec<String>, StorageError> {
        Ok(self.open(Access::Read)?.map(|k| k.value_names()).transpose()?.unwrap_or_default())
    }

    /// The names of the sub-keys (empty when the key does not exist).
    #[cfg(windows)]
    pub fn subkey_names(&self) -> Result<Vec<String>, StorageError> {
        Ok(self.open(Access::Read)?.map(|k| k.subkey_names()).transpose()?.unwrap_or_default())
    }

    /// Reads the key into the cache when it changed (or its properties did). Returns whether the cache changed.
    #[cfg(windows)]
    fn poll(&mut self) -> bool {
        let location = format!("{:?}|{}|{:?}", self.hive, self.path.trim(), self.view);
        let key = match self.open(Access::Read) {
            Ok(k) => k,
            Err(e) => {
                if self.last_error.is_none() {
                    tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), "the Registry key cannot be read: {e}");
                }
                self.last_error = Some(e.to_string());
                return false;
            }
        };
        let stamp = key.as_ref().and_then(|k| k.last_write_time().ok());
        if let Some((loc, st, _)) = &self.cache {
            if *loc == location && *st == stamp {
                return false;
            }
        }
        let mut values = std::collections::HashMap::new();
        if let Some(k) = &key {
            for name in k.value_names().unwrap_or_default() {
                let cached = match k.get_value(&name) {
                    Ok(Some(RegValue::DWord(d))) => Cached::Number(f64::from(d)),
                    Ok(Some(RegValue::QWord(q))) => Cached::Number(q as f64),
                    Ok(Some(RegValue::MultiString(l))) => Cached::Lines(l),
                    Ok(Some(v)) => Cached::Text(v.to_text()),
                    _ => continue,
                };
                values.insert(name, cached);
            }
        }
        let first = self.cache.as_ref().is_none_or(|(loc, _, _)| *loc != location);
        self.cache = Some((location, stamp, values));
        self.last_error = None;
        if !first {
            emit(&mut self.base, "RegistryKey", &self.value_changed, "OnValueChanged", RegistryValueChangedEventArgs { value_name: String::new(), external: true });
        }
        true
    }
}

impl Component for RegistryKey {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// A path is a value name; the component itself (`{Binding Source=key}`) is whether the key exists.
impl BindingProvider for RegistryKey {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        let (_, stamp, values) = self.cache.as_ref()?;
        if path.is_empty() {
            return kubuno_desktop_views::format::to_target(Value::Bool(stamp.is_some()), want, format);
        }
        let v = match values.get(path)? {
            Cached::Text(t) => Value::Str(t.clone()),
            Cached::Number(n) => Value::F32(*n as f32),
            Cached::Lines(l) => Value::Str(l.join("\n")),
        };
        kubuno_desktop_views::format::to_target(v, want, format)
    }

    fn binding_set(&mut self, path: &str, value: Value, format: &BindingFormat, _scope: &ComponentScope) -> bool {
        #[cfg(windows)]
        {
            let value = kubuno_desktop_views::format::from_target(value, format);
            // Keep the type the value has (a DWORD stays a DWORD), REG_SZ for a new one.
            let existing = self.cache.as_ref().and_then(|(_, _, v)| v.get(path).cloned());
            let reg = match (existing, value) {
                (Some(Cached::Number(_)), Value::F32(f)) if f >= 0.0 && f <= u32::MAX as f32 => RegValue::DWord(f.round() as u32),
                (Some(Cached::Number(_)), Value::Bool(b)) => RegValue::DWord(u32::from(b)),
                (Some(Cached::Number(_)), Value::Str(s)) => match s.trim().parse::<u32>() {
                    Ok(d) => RegValue::DWord(d),
                    Err(_) => return true,
                },
                (Some(Cached::Lines(_)), Value::Str(s)) => RegValue::MultiString(s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()),
                (_, Value::Str(s)) => RegValue::String(s),
                (_, Value::F32(f)) => RegValue::String(f.to_string()),
                (_, Value::Bool(b)) => RegValue::String(b.to_string()),
                _ => return true,
            };
            match self.set_value(path, &reg) {
                Ok(()) => {
                    emit(&mut self.base, "RegistryKey", &self.value_changed, "OnValueChanged", RegistryValueChangedEventArgs { value_name: path.to_string(), external: false });
                }
                Err(e) => {
                    tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), value = path, "the Registry value was not written: {e}");
                    self.last_error = Some(e.to_string());
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (path, value, format);
        }
        true
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        if crate::designing(self.design_mode()) || self.path.trim().is_empty() {
            return false;
        }
        let stale = self.cache.is_none() || self.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY);
        if !stale {
            return false;
        }
        self.last_poll = Some(Instant::now());
        #[cfg(windows)]
        {
            self.poll()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}
