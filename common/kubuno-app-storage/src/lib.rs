//! # `kubuno-app-storage` — where a Kubuno app keeps its data on the device
//!
//! vskubuno `docs/STORAGE-COMPONENTS.md`. The portable engine of the storage components (`<Settings>`,
//! `<SecretStore>`, `<RegistryKey>` of `kubuno-app-storage-components`), usable from plain Rust too:
//!
//! - [`paths`]: the per-user and machine directories (`%APPDATA%`, XDG, `~/Library`…), sandbox-aware
//!   (`KUBUNO_SANDBOX_DIR`);
//! - [`Settings`]: typed settings with user/application scope, roaming or local, defaults, change notifications
//!   and versioned upgrade, over a [`backend`] picked per platform or explicitly (files everywhere, the Windows
//!   Registry, memory);
//! - [`AppSecrets`]: the app's secrets in the OS credential store (`kubuno-secrets`), scoped to the app;
//! - [`registry`] (Windows): the Registry with its hives, WOW64 views and typed values, redirected in a sandbox.
//!
//! Every piece of data is scoped to an [`AppId`] (module isolation). Nothing here logs a value.
//!
//! ```
//! use kubuno_app_storage::{AppId, Settings, SettingsOptions, SettingDef, SettingsSchema};
//! use kubuno_app_storage::backend::BackendKind;
//!
//! let schema = SettingsSchema::new("settings", 1)
//!     .with(SettingDef::new("Theme", "System").one_of(&["System", "Light", "Dark"]))
//!     .with(SettingDef::new("RecentFiles", Vec::<String>::new()).local());
//! let app = AppId::new("doc-example").unwrap();
//! let settings = Settings::open(&app, schema, SettingsOptions { backend: BackendKind::Memory, ..Default::default() }).unwrap();
//! settings.set("Theme", "Dark").unwrap();
//! assert_eq!(settings.get_as::<String>("Theme").as_deref(), Some("Dark"));
//! settings.save().unwrap();
//! ```

pub mod account;
pub mod app;
pub mod backend;
mod error;
pub mod files;
pub mod kv;
pub mod paths;
#[cfg(windows)]
pub mod registry;
pub mod secrets;
pub mod settings;

pub use account::{current_account, set_current_account, AccountKey};
pub use app::{default_app_id, set_default_app_id, AppId};
pub use files::{FileInfo, FileKind, FileStore};
pub use kv::{KeyValueStore, Persistence};
pub use error::StorageError;
pub use secrets::{AppSecrets, Secret};
pub use settings::{
    ChangeOrigin, FromSetting, Layer, SettingChange, SettingDef, SettingScope, SettingType, SettingValue, Settings, SettingsOptions, SettingsSchema, Subscription, Upgrade,
};
