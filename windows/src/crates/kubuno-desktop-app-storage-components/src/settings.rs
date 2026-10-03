//! `<Settings>`: an app's settings in a view (WinForms' `ApplicationSettingsBase` dropped on a form).
//!
//! ```xml
//! <Settings x:Name="settings" Schema="settings" OnSettingChanged="settings_setting_changed"/>
//! <Dropdown SelectedValue="{Binding Theme, Source=settings, Mode=TwoWay}"> … </Dropdown>
//! ```
//!
//! The component opens the **shared** settings of its app and set (`kubuno_desktop_app_storage::Settings::shared`): the
//! typed class of the project's `.kbsettings` (`settings!`), the other views and background code all see the same
//! values. The declarations come from that typed class (it registers them before `main`); a set nobody declared
//! is *open* (any name, no default). Paths below the component's name are setting names; each frame the component
//! saves what bindings changed (`AutoSave`), and every two seconds it reloads what another instance changed.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use kubuno_desktop_app_storage::backend::BackendKind;
use kubuno_desktop_app_storage::settings::{registered_schema, registered_schema_of};
use kubuno_desktop_app_storage::{default_app_id, AppId, ChangeOrigin, SettingChange, SettingValue, Settings as Store, SettingsOptions, SettingsSchema, StorageError, Subscription};
use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::events::Event;
use kubuno_desktop_views::format::ValueKind;
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::scope::{BindingProvider, ComponentScope};

use crate::args::{emit, SettingChangedEventArgs, SettingsSavingEventArgs};
use crate::convert::{from_view, to_view};

/// How often a component looks for changes made by another process.
const REFRESH_EVERY: Duration = Duration::from_secs(2);

/// Where the values are stored (`Backend=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum StorageBackend {
    /// The platform's default: JSON files in the user's profile on every OS.
    #[default]
    Auto,
    /// JSON files (`%APPDATA%`, `%LOCALAPPDATA%`, XDG, `~/Library`).
    File,
    /// The Windows Registry (`HKCU\Software\Kubuno\Apps\<app>`); not available elsewhere.
    Registry,
    /// Nothing persists (tests, samples).
    Memory,
}

impl StorageBackend {
    pub fn kind(self) -> BackendKind {
        match self {
            StorageBackend::Auto => BackendKind::Auto,
            StorageBackend::File => BackendKind::File,
            StorageBackend::Registry => BackendKind::Registry,
            StorageBackend::Memory => BackendKind::Memory,
        }
    }
}

/// What the store was opened for: reopened when one of these changes.
#[derive(Debug, Clone, PartialEq)]
struct Opened {
    schema: String,
    app_id: String,
    backend: StorageBackend,
    design: bool,
    account_scoped: bool,
    /// `kubuno_desktop_app_storage::account::account_generation()` when opened: an account switch reopens.
    account_generation: u64,
    /// In the designer, the view's folder the declared schema was read from (a view loaded again from another folder,
    /// or before the host gave its folder, reopens).
    design_folder: Option<std::path::PathBuf>,
}

/// The app's settings (a .kbsettings file of the project): typed values with defaults, bound with {Binding Theme, Source=settings, Mode=TwoWay} and saved in the user's profile.
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "settings-2", category = "Storage")]
#[default_event("SettingChanged")]
#[default_property("Schema")]
pub struct Settings {
    base: ComponentCore,
    /// The settings set: the name of the project's .kbsettings file without its extension (settings for settings.kbsettings).
    #[property]
    #[category("Storage")]
    #[default_value("settings")]
    pub schema: String,
    /// The app the values belong to (its id, kubuno-notes); empty for the application's own.
    #[property]
    #[category("Storage")]
    pub app_id: String,
    /// Where the values are stored: Auto (files of the user's profile), File, Registry (Windows), Memory.
    #[property]
    #[category("Storage")]
    #[default_value("Auto")]
    pub backend: StorageBackend,
    /// Whether a value changed through a binding is saved at once (else call save()).
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub auto_save: bool,
    /// Whether the values belong to the signed-in account (each account its own) rather than to the app; a .kbsettings file can also declare it (AccountScoped).
    #[property]
    #[category("Storage")]
    #[default_value(false)]
    pub account_scoped: bool,
    /// Occurs when a setting's value changed, here or in another instance of the app.
    #[event]
    #[category("Property Changed")]
    pub setting_changed: Event<SettingChangedEventArgs>,
    /// Occurs before the changed values are saved (cancelable: they stay pending).
    #[event]
    #[category("Data")]
    pub settings_saving: Event<SettingsSavingEventArgs>,
    store: Option<(Opened, Store)>,
    /// The changes the store reported, raised as `SettingChanged` on the UI thread.
    changes: Arc<Mutex<Vec<SettingChange>>>,
    subscription: Option<Subscription>,
    seen_generation: u64,
    last_refresh: Option<Instant>,
    last_error: Option<String>,
    /// In the designer (which never syncs the providers), the declared schema read for (view folder, set).
    design_schema: std::cell::RefCell<Option<DesignSchema>>,
}

/// The declared schema of a set as read for the designer: the view folder and set it was read for, and what was found.
type DesignSchema = (Option<std::path::PathBuf>, String, Option<SettingsSchema>);

impl Default for Settings {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            schema: "settings".to_string(),
            app_id: String::new(),
            backend: StorageBackend::Auto,
            auto_save: true,
            account_scoped: false,
            setting_changed: Event::default(),
            settings_saving: Event::default(),
            store: None,
            changes: Arc::new(Mutex::new(Vec::new())),
            subscription: None,
            seen_generation: 0,
            last_refresh: None,
            last_error: None,
            design_schema: std::cell::RefCell::new(None),
        }
    }
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings").field("schema", &self.schema).field("app_id", &self.app_id).field("backend", &self.backend).finish()
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// The settings of set `schema` (`Schema`).
    pub fn with_schema(schema: &str) -> Self {
        Self { schema: schema.to_string(), ..Self::default() }
    }

    fn opened_key(&self) -> Opened {
        Opened {
            schema: self.schema.trim().to_string(),
            app_id: self.app_id.trim().to_string(),
            backend: self.backend,
            design: crate::designing(self.design_mode()),
            account_scoped: self.account_scoped,
            account_generation: kubuno_desktop_app_storage::account::account_generation(),
            design_folder: if crate::designing(self.design_mode()) { kubuno_desktop_views::icon::view_folder() } else { None },
        }
    }

    /// The app, schema and store this component's properties name.
    fn open(key: &Opened) -> Result<Store, StorageError> {
        let set = if key.schema.is_empty() { "settings" } else { key.schema.as_str() };
        let (app, schema) = if key.app_id.is_empty() {
            match registered_schema(set) {
                Some((app, schema)) => (app, schema),
                None => (default_app_id(), SettingsSchema::open(set)),
            }
        } else {
            let app = AppId::new(&key.app_id)?;
            let schema = registered_schema_of(&app, set).unwrap_or_else(|| SettingsSchema::open(set));
            (app, schema)
        };
        let mut schema = schema;
        if key.design {
            // The designer shows the declared defaults (read from the project's .kbsettings beside the view, the typed
            // class not being linked into the surface yet), and never reads or writes the developer's own settings.
            if schema.open {
                if let Some((_, declared)) = kubuno_desktop_views::icon::view_folder().and_then(|dir| crate::design::schema_from_project(&dir, set)) {
                    schema = declared;
                }
            }
            schema.account_scoped = false;
            return Store::open(&app, schema, SettingsOptions { backend: BackendKind::Memory, ..Default::default() });
        }
        schema.account_scoped |= key.account_scoped;
        Store::shared(&app, &schema, key.backend.kind())
    }

    /// The declared default of setting `name` in the project's `.kbsettings` (the designer, before any store is opened).
    fn declared_default(&self, name: &str) -> Option<SettingValue> {
        let folder = kubuno_desktop_views::icon::view_folder();
        let set = if self.schema.trim().is_empty() { "settings".to_string() } else { self.schema.trim().to_string() };
        let mut cache = self.design_schema.try_borrow_mut().ok()?;
        if !matches!(&*cache, Some((f, s, _)) if *f == folder && *s == set) {
            let schema = registered_schema(&set).map(|(_, s)| s).filter(|s| !s.open).or_else(|| folder.as_deref().and_then(|dir| crate::design::schema_from_project(dir, &set)).map(|(_, s)| s));
            *cache = Some((folder, set, schema));
        }
        let (_, _, schema) = cache.as_ref()?;
        schema.as_ref()?.find(name).map(|d| d.default.clone())
    }

    /// The settings behind the component (opened on first use, reopened when `Schema`, `AppId` or `Backend`
    /// changed). `None` when they cannot be opened (see [`Settings::last_error`]).
    pub fn store(&mut self) -> Option<Store> {
        let key = self.opened_key();
        if let Some((k, s)) = &self.store {
            if *k == key {
                return Some(s.clone());
            }
        }
        match Self::open(&key) {
            Ok(store) => {
                let changes = self.changes.clone();
                self.subscription = Some(store.subscribe(move |c| changes.lock().unwrap_or_else(PoisonError::into_inner).push(c.clone())));
                self.seen_generation = store.generation();
                self.last_error = None;
                self.store = Some((key, store.clone()));
                Some(store)
            }
            Err(e) => {
                tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), "the settings cannot be opened: {e}");
                self.last_error = Some(e.to_string());
                self.store = None;
                None
            }
        }
    }

    /// The effective value of `name`.
    pub fn get_value(&mut self, name: &str) -> Option<SettingValue> {
        self.store()?.get(name)
    }

    /// Changes `name` (saved at the next frame with `AutoSave`, else by [`Settings::save`]).
    pub fn set_value(&mut self, name: &str, value: impl Into<SettingValue>) -> Result<bool, StorageError> {
        let store = self.store().ok_or_else(|| StorageError::Backend { backend: "settings", message: self.last_error.clone().unwrap_or_default() })?;
        store.set(name, value)
    }

    /// Saves the pending changes now (after `SettingsSaving`).
    pub fn save(&mut self) -> Result<(), StorageError> {
        let Some(store) = self.store() else { return Ok(()) };
        if !store.is_dirty() {
            return Ok(());
        }
        let args = emit(&mut self.base, "Settings", &self.settings_saving, "OnSettingsSaving", SettingsSavingEventArgs::default());
        if args.cancel {
            return Ok(());
        }
        let result = store.save();
        self.last_error = result.as_ref().err().map(ToString::to_string);
        result
    }

    /// Reads the values again, dropping unsaved changes.
    pub fn reload(&mut self) {
        if let Some(store) = self.store() {
            store.reload();
        }
    }

    /// Why the settings could not be opened or saved, if they could not.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Raises `SettingChanged` for the changes the store reported.
    fn raise_changes(&mut self) -> bool {
        let pending: Vec<SettingChange> = std::mem::take(&mut *self.changes.lock().unwrap_or_else(PoisonError::into_inner));
        for c in &pending {
            let args = SettingChangedEventArgs { setting_name: c.name.clone(), external: matches!(c.origin, ChangeOrigin::External | ChangeOrigin::Reload) };
            emit(&mut self.base, "Settings", &self.setting_changed, "OnSettingChanged", args);
        }
        !pending.is_empty()
    }
}

impl Component for Settings {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// Paths are setting names (`{Binding Theme, Source=settings}`); a `StringList` is rows with a `Value` field.
impl BindingProvider for Settings {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        // Reading never opens: `binding_sync` (called before the paint) does.
        let Some((_, store)) = self.store.as_ref() else {
            // The design surface never syncs its providers: the declared default, read from the project.
            return if crate::designing(self.design_mode()) { self.declared_default(path).and_then(|v| kubuno_desktop_views::format::to_target(to_view(&v), want, format)) } else { None };
        };
        let v = store.get(path)?;
        kubuno_desktop_views::format::to_target(to_view(&v), want, format)
    }

    fn binding_set(&mut self, path: &str, value: Value, format: &BindingFormat, _scope: &ComponentScope) -> bool {
        let Some(store) = self.store() else { return true };
        let ty = store.schema().find(path).map(|d| d.ty);
        let value = kubuno_desktop_views::format::from_target(value, format);
        match from_view(value, ty) {
            Some(v) => {
                if let Err(e) = store.set(path, v) {
                    tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), setting = path, "the value was refused: {e}");
                    self.last_error = Some(e.to_string());
                }
            }
            None => tracing::warn!(target: "kubuno_desktop_app_storage", component = %self.display_name(), setting = path, "the value does not convert to the setting's type"),
        }
        true
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        let Some(store) = self.store() else { return false };
        if self.auto_save && store.is_dirty() {
            let _ = self.save();
        }
        let due = self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH_EVERY);
        if due && !crate::designing(self.design_mode()) {
            self.last_refresh = Some(Instant::now());
            store.refresh_if_changed();
        }
        let raised = self.raise_changes();
        let generation = store.generation();
        let changed = generation != self.seen_generation;
        self.seen_generation = generation;
        changed || raised
    }
}
