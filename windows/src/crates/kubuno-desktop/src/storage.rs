//! Storage, the Kubuno way (`vskubuno/docs/STORAGE-COMPONENTS.md`): typed settings, the app's secrets and the
//! Windows Registry, as the `<Settings>`, `<SecretStore>` and `<RegistryKey>` components of a view and from code.
//!
//! Typed settings come from a `.kbsettings` file of the project (Windows Forms' `Settings.settings`):
//!
//! ```ignore
//! kubuno_desktop::settings!("settings.kbsettings");          // pub struct Settings, one accessor per setting
//!
//! fn main_view_load(&mut self, _sender: &Control, _e: &EventArgs) {
//!     self.zoom.set_value(Settings::zoom() as f32);
//!     Settings::set_last_opened(chrono_now());       // saved at once
//! }
//! ```
//!
//! A view binds the same values through its `<Settings>` component:
//! `<Switch On="{Binding ShowHidden, Source=settings, Mode=TwoWay}"/>`. The `x:Name`d components are fields of
//! the form typed with the handles below (`self.settings.get_as::<bool>("ShowHidden")`,
//! `self.secrets.set_str("ApiKey", key)`, `self.run_key.get_string("Kubuno")`).
//!
//! The types here are cheap, clonable **handles**, like the controls of [`crate::forms`]: a component of a
//! `.kbview`, reached through its window, or a component created in code. The component classes are
//! [`components`]; the portable engine (paths, `Settings`, back-ends, `AppSecrets`, the Registry) is re-exported
//! at this module's root (`kubuno_desktop::storage::Settings` is the handle; the engine's is [`engine::Settings`]).

use std::cell::RefCell;
use std::rc::Rc;

use crate::forms::{AsControl, Control};

pub use kubuno_desktop_app_storage_components::engine::{
    app, backend, account, files, kv, FileInfo, FileKind, AccountKey, set_current_account, current_account, default_app_id, paths, secrets, set_default_app_id, settings, AppId, AppSecrets, ChangeOrigin, FromSetting, Layer, Secret, SettingChange, SettingDef,
    SettingScope, SettingType, SettingValue, SettingsOptions, SettingsSchema, StorageError, Subscription, Upgrade,
};
#[cfg(windows)]
pub use kubuno_desktop_app_storage_components::engine::registry;
pub use kubuno_desktop_app_storage_components::{
    FileStoreKind, KeyValuePersistence, RegistryHive, RegistryKeyView, RegistryValueChangedEventArgs, SecretBackend, SettingChangedEventArgs, SettingsSavingEventArgs, StorageBackend,
};

/// The portable engine (`kubuno-desktop-app-storage`): `engine::Settings` is the shared settings object itself.
pub use kubuno_desktop_app_storage_components::engine;

/// The component classes (`<Settings>`… of a `.kbview`), for code that works with the view runtime directly
/// (`runtime.with_component::<components::Settings, _>(…)`).
pub mod components {
    pub use kubuno_desktop_app_storage_components::{FileStore, KeyValueStore, RegistryKey, SecretStore, Settings};
}

/// The error of a component of a view that cannot be reached now (its window is not open, or it is busy).
fn unreachable(element: &str) -> StorageError {
    StorageError::Backend { backend: "view", message: format!("the {element} of the view cannot be reached now (its window is not open, or it is busy)") }
}

macro_rules! handle {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            control: Control,
            own: Rc<RefCell<kubuno_desktop_app_storage_components::$name>>,
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.control.get_name())
            }
        }

        impl AsControl for $name {
            fn as_control(&self) -> &Control {
                &self.control
            }
        }

        impl $name {
            /// The element's name in the `.kbview`.
            pub const ELEMENT: &'static str = stringify!($name);

            /// A component created in code.
            pub fn new() -> Self {
                Self { control: Control::new(stringify!($name)), own: Rc::new(RefCell::new(kubuno_desktop_app_storage_components::$name::default())) }
            }

            /// Runs `f` on the component (`None`: a component of a view not reachable now).
            pub fn with<R>(&self, f: impl FnOnce(&mut kubuno_desktop_app_storage_components::$name) -> R) -> Option<R> {
                if self.control.0.from_view.get() && !self.control.get_name().is_empty() {
                    let out = self.control.with::<kubuno_desktop_app_storage_components::$name, R>(f);
                    if out.is_none() {
                        tracing::warn!(name = %self.control.get_name(), concat!(stringify!($name), " of the view cannot be reached now (its window is not open, or it is busy)"));
                    }
                    return out;
                }
                match self.own.try_borrow_mut() {
                    Ok(mut c) => Some(f(&mut c)),
                    Err(_) => {
                        tracing::warn!(concat!(stringify!($name), " is busy (reached from one of its own events)"));
                        None
                    }
                }
            }

            fn try_with<R>(&self, f: impl FnOnce(&mut kubuno_desktop_app_storage_components::$name) -> Result<R, StorageError>) -> Result<R, StorageError> {
                self.with(f).unwrap_or_else(|| Err(unreachable(stringify!($name))))
            }
        }
    };
}

handle!(
    /// `<Settings>`: the settings of a set of the app (see the module doc). Created in code:
    /// `Settings::new().schema("window")`.
    Settings
);

impl Settings {
    /// The settings set (`Schema`), builder form.
    pub fn schema(self, set: &str) -> Self {
        self.with(|s| s.schema = set.to_string());
        self
    }

    /// The app the values belong to (`AppId`), builder form.
    pub fn app_id(self, id: &str) -> Self {
        self.with(|s| s.app_id = id.to_string());
        self
    }

    /// Where the values are stored (`Backend`), builder form.
    pub fn backend(self, backend: StorageBackend) -> Self {
        self.with(|s| s.backend = backend);
        self
    }

    /// The shared settings behind the component.
    pub fn store(&self) -> Option<engine::Settings> {
        self.with(|s| s.store()).flatten()
    }

    /// The effective value of `name`.
    pub fn get(&self, name: &str) -> Option<SettingValue> {
        self.store()?.get(name)
    }

    /// The value of `name` as a Rust type (`get_as::<bool>("ShowHidden")`).
    pub fn get_as<T: FromSetting>(&self, name: &str) -> Option<T> {
        self.store()?.get_as(name)
    }

    /// Changes `name` (the component saves it at the next frame with `AutoSave`; call [`Self::save`] otherwise).
    pub fn set(&self, name: &str, value: impl Into<SettingValue>) -> Result<bool, StorageError> {
        let value = value.into();
        self.try_with(|s| s.set_value(name, value))
    }

    /// Saves the pending changes now.
    pub fn save(&self) -> Result<(), StorageError> {
        self.try_with(|s| s.save())
    }

    /// Reads the values again, dropping unsaved changes.
    pub fn reload(&self) {
        self.with(|s| s.reload());
    }

    /// Why the settings could not be opened or saved, if they could not.
    pub fn last_error(&self) -> Option<String> {
        self.with(|s| s.last_error().map(str::to_string)).flatten()
    }
}

handle!(
    /// `<SecretStore>`: the app's secrets in the OS credential store. A secret never goes through a binding:
    /// read and write it here.
    SecretStore
);

impl SecretStore {
    /// The app the secrets belong to (`AppId`), builder form.
    pub fn app_id(self, id: &str) -> Self {
        self.with(|s| s.app_id = id.to_string());
        self
    }

    /// Where the secrets are kept (`Backend`), builder form.
    pub fn backend(self, backend: SecretBackend) -> Self {
        self.with(|s| s.backend = backend);
        self
    }

    /// The secret `name`, `None` when there is none.
    pub fn get(&self, name: &str) -> Result<Option<Secret>, StorageError> {
        self.try_with(|s| s.get(name))
    }

    pub fn set(&self, name: &str, value: &Secret) -> Result<(), StorageError> {
        self.try_with(|s| s.set(name, value))
    }

    /// Stores a text secret.
    pub fn set_str(&self, name: &str, value: &str) -> Result<(), StorageError> {
        self.try_with(|s| s.set_str(name, value))
    }

    /// Removes the secret; `Ok(false)` when there was none.
    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        self.try_with(|s| s.delete(name))
    }

    pub fn contains(&self, name: &str) -> Result<bool, StorageError> {
        self.try_with(|s| s.contains(name))
    }
}

handle!(
    /// `<RegistryKey>` (Windows): one key of the Registry. Created in code:
    /// `RegistryKey::new().hive(RegistryHive::CurrentUser).path(r"Software\Contoso")`.
    RegistryKey
);

impl RegistryKey {
    /// The root key (`Hive`), builder form.
    pub fn hive(self, hive: RegistryHive) -> Self {
        self.with(|k| k.hive = hive);
        self
    }

    /// The key's path below its root (`Path`), builder form.
    pub fn path(self, path: &str) -> Self {
        self.with(|k| k.path = path.to_string());
        self
    }

    /// The WOW64 view (`View`), builder form.
    pub fn view(self, view: RegistryKeyView) -> Self {
        self.with(|k| k.view = view);
        self
    }

    /// Whether values may be written (`Writable`), builder form.
    pub fn writable(self, on: bool) -> Self {
        self.with(|k| k.writable = on);
        self
    }

    /// `HKCU\Software\…` (the sandbox's copy in a sandboxed profile).
    pub fn full_name(&self) -> String {
        self.with(|k| k.full_name()).unwrap_or_default()
    }

    pub fn exists(&self) -> Result<bool, StorageError> {
        self.try_with(|k| k.exists())
    }

    /// The value `name` as text (numbers in decimal, lists one item per line); `None` when it does not exist.
    pub fn get_string(&self, name: &str) -> Result<Option<String>, StorageError> {
        self.try_with(|k| k.get_string(name))
    }

    /// The value `name`, typed.
    #[cfg(windows)]
    pub fn get_value(&self, name: &str) -> Result<Option<registry::RegValue>, StorageError> {
        self.try_with(|k| k.get_value(name))
    }

    /// Writes the value `name` (refused unless `Writable`).
    #[cfg(windows)]
    pub fn set_value(&self, name: &str, value: &registry::RegValue) -> Result<(), StorageError> {
        self.try_with(|k| k.set_value(name, value))
    }

    /// Removes the value `name` (refused unless `Writable`).
    #[cfg(windows)]
    pub fn delete_value(&self, name: &str) -> Result<bool, StorageError> {
        self.try_with(|k| k.delete_value(name))
    }

    #[cfg(windows)]
    pub fn value_names(&self) -> Result<Vec<String>, StorageError> {
        self.try_with(|k| k.value_names())
    }

    #[cfg(windows)]
    pub fn subkey_names(&self) -> Result<Vec<String>, StorageError> {
        self.try_with(|k| k.subkey_names())
    }
}

handle!(
    /// `<KeyValueStore>`: small untyped values of the app (a last-opened folder, a dismissed tip), written at once.
    KeyValueStore
);

impl KeyValueStore {
    /// The store's name (`Store`), builder form.
    pub fn store(self, name: &str) -> Self {
        self.with(|s| s.store = name.to_string());
        self
    }

    /// Whether the values belong to the signed-in account (`AccountScoped`), builder form.
    pub fn account_scoped(self, on: bool) -> Self {
        self.with(|s| s.account_scoped = on);
        self
    }

    /// The value of `key` (`None`: absent or expired).
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.with(|s| s.get(key)).flatten()
    }

    /// The value of `key` as text.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.with(|s| s.get_string(key)).flatten()
    }

    /// Sets `key` (written at once), optionally expiring after `ttl`.
    pub fn set(&self, key: &str, value: impl Into<serde_json::Value>, ttl: Option<std::time::Duration>) -> Result<(), StorageError> {
        let value = value.into();
        self.try_with(|s| s.set(key, value, ttl))
    }

    pub fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.try_with(|s| s.remove(key))
    }

    pub fn keys(&self) -> Vec<String> {
        self.with(|s| s.keys()).unwrap_or_default()
    }
}

handle!(
    /// `<FileStore>`: the app's own files by name — data, a cache with eviction, temporary files.
    FileStore
);

impl FileStore {
    /// The store's folder name (`Folder`), builder form.
    pub fn folder(self, name: &str) -> Self {
        self.with(|s| s.folder = name.to_string());
        self
    }

    /// Data, Cache or Temp (`Kind`), builder form.
    pub fn kind(self, kind: FileStoreKind) -> Self {
        self.with(|s| s.kind = kind);
        self
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.try_with(|s| s.write(name, bytes))
    }

    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.try_with(|s| s.read(name))
    }

    pub fn read_to_string(&self, name: &str) -> Result<Option<String>, StorageError> {
        self.try_with(|s| s.read_to_string(name))
    }

    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        self.try_with(|s| s.delete(name))
    }

    pub fn list(&self) -> Result<Vec<FileInfo>, StorageError> {
        self.try_with(|s| s.list())
    }

    /// The path of `name` in the store, for an API that takes a path.
    pub fn path_of(&self, name: &str) -> Result<std::path::PathBuf, StorageError> {
        self.try_with(|s| s.path_of(name))
    }

    pub fn clear(&self) -> Result<usize, StorageError> {
        self.try_with(|s| s.clear())
    }
}
