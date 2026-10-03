//! # `kubuno-desktop-app-storage-components` — storage components of Kubuno desktop views
//!
//! `vskubuno/docs/STORAGE-COMPONENTS.md`. Non-visual components of a `.kbview` (the designer's component tray, the
//! Toolbox tab "Stockage" / "Storage") over the portable engine `kubuno-desktop-app-storage`:
//!
//! ```xml
//! <Settings x:Name="settings" Schema="settings" OnSettingChanged="settings_setting_changed"/>
//! <SecretStore x:Name="secrets"/>
//! <RegistryKey x:Name="explorer_key" Hive="CurrentUser" Path="Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"/>
//!
//! <Switch On="{Binding ShowHidden, Source=settings, Mode=TwoWay}"/>
//! <Dropdown SelectedValue="{Binding Theme, Source=settings, Mode=TwoWay}"/>
//! <Label Text="{Binding Hidden, Source=explorer_key}"/>
//! <Label Visible="{Binding ApiKey.Exists, Source=secrets}" Text="Clé enregistrée"/>
//! ```
//!
//! - [`Settings`]: the settings set `Schema` (a `.kbsettings` file of the project, whose typed class `settings!`
//!   generates and registers) of the app; every binding reads and writes the same shared values as the typed class;
//!   changes are saved at once (`AutoSave`), other instances' changes are picked up every two seconds.
//! - [`SecretStore`]: the app's secrets in the OS credential store; bindings only tell whether a secret exists,
//!   never its value.
//! - [`RegistryKey`] (Windows): one Registry key, its values bindable by name; read-only unless `Writable`.
//!
//! In the designer (and with `Backend="Memory"`), nothing touches the user's profile: settings are in memory with
//! their defaults, the secret store is in memory, the Registry is not read. In a sandboxed profile
//! (`KUBUNO_SANDBOX_DIR`) every location, secret and Registry key is the sandbox's.
//!
//! An application links the components' registrations (static constructors) through the `kubuno-desktop` crate
//! (`kubuno_desktop::storage`), or with `extern crate kubuno_desktop_app_storage_components as _;`.

pub mod args;
mod convert;
mod design;
pub mod file_store;
pub mod key_value_store;
pub mod registry_key;
pub mod secret_store;
pub mod settings;

pub use args::{RegistryValueChangedEventArgs, SettingChangedEventArgs, SettingsSavingEventArgs};
pub use kubuno_desktop_app_storage as engine;
pub use file_store::{FileStore, FileStoreKind};
pub use key_value_store::{KeyValuePersistence, KeyValueStore};
pub use registry_key::{RegistryHive, RegistryKey, RegistryKeyView};
pub use secret_store::{SecretBackend, SecretStore};
pub use settings::{Settings, StorageBackend};

/// The element names this crate registers (`kubuno_desktop_views_meta::kbview::STORAGE_ELEMENTS`, kept equal by a test).
pub const ELEMENTS: &[&str] = &["FileStore", "KeyValueStore", "RegistryKey", "SecretStore", "Settings"];

/// Whether the component must leave the user's profile alone: hosted by the designer, or in a process marked as
/// the design surface.
pub(crate) fn designing(site_design_mode: bool) -> bool {
    site_design_mode || kubuno_desktop_views::design::design_time()
}
