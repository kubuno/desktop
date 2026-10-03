//! [`Settings`]: the engine behind the `<Settings>` component and the `settings!` typed classes.
//!
//! - **Typed**: every value goes through its [`SettingDef`] (type, accepted values, limits); a stored value that
//!   does not fit (edited by hand, written by another version) reads as the default, and is left in the store.
//! - **Scoped**: user settings in the roaming or the local layer, application settings in the machine layer
//!   (read-only); a user setting the user never changed reads the machine layer (an administrator's default), then
//!   the schema's default.
//! - **Change notifications**: [`Settings::subscribe`] (any thread) and [`Settings::generation`] (a counter a UI
//!   polls once per frame); external changes (another instance, an administrator) are noticed by
//!   [`Settings::refresh_if_changed`].
//! - **Upgrade**: a stored layer written for an older schema version is upgraded when it is loaded: renamed
//!   settings ([`SettingDef::previous_names`]) are moved, then the app's own [`SettingsOptions::upgrade`] runs; the
//!   stored version is raised. A layer written by a *newer* version is read as it is and never downgraded.
//! - **Saving** is explicit ([`Settings::save`], WinForms' `Save()`); it writes only what changed, merged into what
//!   the store holds at that moment.
//! - **Shared instances** ([`Settings::shared`]): one per app, set and back-end in a process, so a typed class, the
//!   components of every open view and background code see the same values at once.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use super::schema::{check_limits, Layer, SettingDef, SettingScope, SettingsSchema};
use super::value::{FromSetting, SettingValue};
use crate::app::AppId;
use crate::account::AccountKey;
use crate::backend::{BackendKind, FileBackend, FileRoots, MemoryBackend, SettingsBackend};
use crate::StorageError;

/// The app's own upgrade step (see [`Upgrade`]).
pub type UpgradeFn = Arc<dyn Fn(&mut Upgrade<'_>) + Send + Sync>;

/// How [`Settings::open`] works.
#[derive(Clone, Default)]
pub struct SettingsOptions {
    pub backend: BackendKind,
    /// Allows writing application-scoped settings (the machine layer): installers and administration tools only.
    pub allow_machine_writes: bool,
    /// Runs after the built-in renames when a stored layer is older than the schema.
    pub upgrade: Option<UpgradeFn>,
    /// The account of an account-scoped schema (`None`: the current account, `crate::account::current_account`).
    pub account: Option<AccountKey>,
}

impl fmt::Debug for SettingsOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsOptions").field("backend", &self.backend).field("allow_machine_writes", &self.allow_machine_writes).field("upgrade", &self.upgrade.is_some()).field("account", &self.account).finish()
    }
}

/// One stored layer being upgraded from an older schema version (WinForms' `Upgrade()` / `SettingsUpgrade`
/// event, but run in place: no per-version copies of the file).
pub struct Upgrade<'a> {
    from: u32,
    to: u32,
    layer: Layer,
    values: &'a mut BTreeMap<String, SettingValue>,
    changes: &'a mut BTreeMap<String, Option<SettingValue>>,
}

impl Upgrade<'_> {
    /// The version the layer was written for (0: written before versions were recorded).
    pub fn from_version(&self) -> u32 {
        self.from
    }

    pub fn to_version(&self) -> u32 {
        self.to
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    pub fn get(&self, name: &str) -> Option<&SettingValue> {
        self.values.get(name)
    }

    pub fn set(&mut self, name: &str, value: impl Into<SettingValue>) {
        let v = value.into();
        self.values.insert(name.to_string(), v.clone());
        self.changes.insert(name.to_string(), Some(v));
    }

    pub fn remove(&mut self, name: &str) -> Option<SettingValue> {
        let old = self.values.remove(name);
        if old.is_some() {
            self.changes.insert(name.to_string(), None);
        }
        old
    }

    /// Moves the value of `old` to `new` (unless `new` already has one); `true` when something moved.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if self.values.contains_key(new) || !self.values.contains_key(old) {
            return false;
        }
        match self.remove(old) {
            Some(v) => {
                self.set(new, v);
                true
            }
            None => false,
        }
    }
}

/// Why a value changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOrigin {
    /// [`Settings::set`] in this process.
    Set,
    /// [`Settings::reset`] / [`Settings::reset_all`].
    Reset,
    /// [`Settings::reload`].
    Reload,
    /// Written by another process and noticed by [`Settings::refresh_if_changed`].
    External,
}

/// A change of a setting's effective value.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingChange {
    pub name: String,
    pub old: Option<SettingValue>,
    pub new: Option<SettingValue>,
    pub origin: ChangeOrigin,
}

type Callback = Arc<dyn Fn(&SettingChange) + Send + Sync>;

/// The process-wide instances of [`Settings::shared`], by app, set and back-end.
type SharedMap = HashMap<(AppId, String, BackendKind, Option<AccountKey>), Settings>;

/// Keeps a [`Settings::subscribe`] callback registered; dropping it unsubscribes.
#[must_use = "the callback is removed when the subscription is dropped"]
pub struct Subscription {
    inner: Weak<Inner>,
    id: u64,
}

impl Subscription {
    /// Keeps the callback for the life of the settings (an app-wide listener).
    pub fn forget(mut self) {
        self.inner = Weak::new();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.subscribers.lock().unwrap_or_else(PoisonError::into_inner).retain(|(id, _)| *id != self.id);
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Subscription({})", self.id)
    }
}

#[derive(Default)]
struct State {
    stored: BTreeMap<Layer, BTreeMap<String, SettingValue>>,
    dirty: BTreeMap<(Layer, String), Option<SettingValue>>,
    stamps: BTreeMap<Layer, Option<u64>>,
    /// The last load failure of each layer (the location and the error, never content).
    errors: BTreeMap<Layer, String>,
}

struct Inner {
    app: AppId,
    schema: SettingsSchema,
    backend: Box<dyn SettingsBackend>,
    options: SettingsOptions,
    state: Mutex<State>,
    subscribers: Mutex<Vec<(u64, Callback)>>,
    next_id: AtomicU64,
    generation: AtomicU64,
}

/// The settings of one app and set (see the module doc). Cheap to clone: clones share the same values.
#[derive(Clone)]
pub struct Settings {
    inner: Arc<Inner>,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings").field("app", &self.inner.app).field("set", &self.inner.schema.set).field("backend", &self.inner.backend.name()).finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The back-end of `kind` for an app's set.
pub fn make_backend(app: &AppId, set: &str, kind: BackendKind, account: Option<&AccountKey>) -> Result<Box<dyn SettingsBackend>, StorageError> {
    Ok(match kind {
        BackendKind::Auto | BackendKind::File => Box::new(FileBackend::new(app, set, match account {
            Some(a) => FileRoots::for_account(a)?,
            None => FileRoots::platform()?,
        })),
        BackendKind::Memory => Box::new(MemoryBackend::new()),
        #[cfg(windows)]
        BackendKind::Registry => Box::new(crate::backend::RegistryBackend::new(app, set).with_account(account.cloned())),
        #[cfg(not(windows))]
        BackendKind::Registry => return Err(StorageError::Unsupported("the Windows Registry")),
    })
}

impl Settings {
    /// Opens the settings of `app` declared by `schema`, on the back-end of `options`, and loads them.
    pub fn open(app: &AppId, schema: SettingsSchema, mut options: SettingsOptions) -> Result<Self, StorageError> {
        if schema.account_scoped && options.account.is_none() {
            options.account = crate::account::current_account();
        }
        let backend: Box<dyn SettingsBackend> = if schema.account_scoped && options.account.is_none() {
            // No account is signed in: the values are kept for the session, never written under another one.
            tracing::warn!(target: "kubuno_app_storage", app = %app, set = %schema.set, "account-scoped settings without a current account are kept in memory");
            Box::new(MemoryBackend::new())
        } else {
            make_backend(app, &schema.set, options.backend, if schema.account_scoped { options.account.as_ref() } else { None })?
        };
        Self::with_backend(app, schema, backend, options)
    }

    /// Like [`Settings::open`] on a back-end of the caller's (tests: a [`MemoryBackend`], a [`FileBackend`] in a
    /// temporary directory, a Registry back-end below a test key).
    pub fn with_backend(app: &AppId, schema: SettingsSchema, backend: Box<dyn SettingsBackend>, options: SettingsOptions) -> Result<Self, StorageError> {
        if !schema.open {
            schema.validate()?;
        } else if !super::schema::valid_set_name(&schema.set) {
            return Err(StorageError::InvalidName(format!("settings set '{}'", schema.set)));
        }
        Ok(Self::build(app, schema, backend, options))
    }

    /// Builds and loads, without validating the schema (the callers did, or fall back to memory).
    fn build(app: &AppId, schema: SettingsSchema, backend: Box<dyn SettingsBackend>, options: SettingsOptions) -> Self {
        let s = Self {
            inner: Arc::new(Inner {
                app: app.clone(),
                schema,
                backend,
                options,
                state: Mutex::new(State::default()),
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                generation: AtomicU64::new(0),
            }),
        };
        s.load(true, None);
        s
    }

    /// The process-wide instance of `app`'s set `schema.set` on `kind` (opened on first use). An instance opened
    /// with an open schema (a component that ran before the typed class registered the real one) is replaced
    /// when the real schema arrives.
    pub fn shared(app: &AppId, schema: &SettingsSchema, kind: BackendKind) -> Result<Self, StorageError> {
        static SHARED: OnceLock<Mutex<SharedMap>> = OnceLock::new();
        let kind = if kind == BackendKind::Auto { BackendKind::File } else { kind };
        let map = SHARED.get_or_init(|| Mutex::new(HashMap::new()));
        let account = if schema.account_scoped { crate::account::current_account() } else { None };
        let key = (app.clone(), schema.set.clone(), kind, account.clone());
        if let Some(existing) = lock(map).get(&key) {
            if !(existing.inner.schema.open && !schema.open) {
                return Ok(existing.clone());
            }
        }
        let opened = Self::open(app, schema.clone(), SettingsOptions { backend: kind, account, ..Default::default() })?;
        let mut m = lock(map);
        // Another thread may have opened it meanwhile: keep the first one, unless it is the open stand-in.
        match m.get(&key) {
            Some(existing) if !(existing.inner.schema.open && !schema.open) => Ok(existing.clone()),
            _ => {
                m.insert(key, opened.clone());
                Ok(opened)
            }
        }
    }

    /// [`Settings::shared`], or settings in memory when the store cannot be opened (no home directory, an
    /// unsupported back-end): the app keeps working on its defaults, and the failure is logged.
    pub fn shared_or_memory(app: &AppId, schema: &SettingsSchema, kind: BackendKind) -> Self {
        match Self::shared(app, schema, kind) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "kubuno_app_storage", app = %app, set = %schema.set, "the settings cannot be opened, they are kept in memory: {e}");
                Self::shared(app, schema, BackendKind::Memory).unwrap_or_else(|_| Self::build(app, schema.clone(), Box::new(MemoryBackend::new()), SettingsOptions::default()))
            }
        }
    }

    pub fn app(&self) -> &AppId {
        &self.inner.app
    }

    pub fn schema(&self) -> &SettingsSchema {
        &self.inner.schema
    }

    /// The back-end's name (`"file"`, `"registry"`, `"memory"`).
    pub fn backend_name(&self) -> &'static str {
        self.inner.backend.name()
    }

    /// Where `layer` is stored.
    pub fn location(&self, layer: Layer) -> String {
        self.inner.backend.location(layer)
    }

    /// Why the last load of `layer` failed (the values then read as their defaults), if it did.
    pub fn load_error(&self, layer: Layer) -> Option<String> {
        lock(&self.inner.state).errors.get(&layer).cloned()
    }

    /// Bumped by every change of an effective value: a UI repaints when it differs from the one it last saw.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// The declared names (an open schema: every stored name).
    pub fn names(&self) -> Vec<String> {
        if !self.inner.schema.open {
            return self.inner.schema.defs.iter().map(|d| d.name.clone()).collect();
        }
        let st = lock(&self.inner.state);
        let mut names: BTreeSet<String> = BTreeSet::new();
        for layer in [self.open_layer(), Layer::Machine] {
            names.extend(st.stored.get(&layer).into_iter().flat_map(|m| m.keys().cloned()));
        }
        names.into_iter().collect()
    }

    /// The effective value of `name`: the user's, else the machine's, else the default. `None` for a name the
    /// schema does not declare (an open schema: one nobody stored).
    pub fn get(&self, name: &str) -> Option<SettingValue> {
        let st = lock(&self.inner.state);
        self.effective(&st, name)
    }

    /// [`Settings::get`] as a Rust type (`None` also when it does not convert).
    pub fn get_as<T: FromSetting>(&self, name: &str) -> Option<T> {
        self.get(name).and_then(|v| T::from_setting(&v))
    }

    /// [`Settings::get_as`], or `T::default()`.
    pub fn value_or_default<T: FromSetting + Default>(&self, name: &str) -> T {
        self.get_as(name).unwrap_or_default()
    }

    /// Whether the app cannot write `name` (application scope, unless machine writes are allowed).
    pub fn is_read_only(&self, name: &str) -> bool {
        self.inner.schema.find(name).is_some_and(|d| d.scope == SettingScope::Application) && !self.inner.options.allow_machine_writes
    }

    /// Whether nothing was ever stored for the user (no value and no version in either user layer): what a one-time
    /// import of an older settings file checks first.
    pub fn is_fresh(&self) -> bool {
        let st = lock(&self.inner.state);
        if !st.dirty.is_empty() {
            return false;
        }
        [Layer::UserRoaming, Layer::UserLocal].iter().all(|l| st.stored.get(l).is_none_or(BTreeMap::is_empty))
            && [Layer::UserRoaming, Layer::UserLocal].iter().all(|l| st.stamps.get(l).copied().flatten().is_none())
    }

    /// Whether a value differs from what is stored (a [`Settings::save`] is due).
    pub fn is_dirty(&self) -> bool {
        !lock(&self.inner.state).dirty.is_empty()
    }

    fn def_for(&self, name: &str, value: Option<&SettingValue>) -> Result<SettingDef, StorageError> {
        if let Some(d) = self.inner.schema.find(name) {
            return Ok(d.clone());
        }
        if self.inner.schema.open && super::schema::valid_setting_name(name) {
            let mut d = SettingDef::new(name, value.cloned().unwrap_or(SettingValue::String(String::new())));
            d.default = d.ty.zero();
            d.roaming = !self.inner.schema.open_local;
            return Ok(d);
        }
        Err(StorageError::Setting { name: name.to_string(), message: format!("not declared by the settings set '{}'", self.inner.schema.set) })
    }

    fn writable_layer(&self, def: &SettingDef) -> Result<Layer, StorageError> {
        let layer = def.layer();
        if layer == Layer::Machine && !self.inner.options.allow_machine_writes {
            return Err(StorageError::ReadOnly(format!("the application-scoped setting '{}'", def.name)));
        }
        Ok(layer)
    }

    /// Changes `name` (in memory: [`Settings::save`] stores it). Returns whether the effective value changed.
    pub fn set(&self, name: &str, value: impl Into<SettingValue>) -> Result<bool, StorageError> {
        let value = value.into();
        let def = self.def_for(name, Some(&value))?;
        let layer = self.writable_layer(&def)?;
        let v = if self.inner.schema.open {
            check_limits(name, &value)?;
            value
        } else {
            def.accept(&value)?
        };
        let change = {
            let mut st = lock(&self.inner.state);
            let old = self.effective(&st, name);
            st.stored.entry(layer).or_default().insert(def.name.clone(), v.clone());
            st.dirty.insert((layer, def.name.clone()), Some(v));
            let new = self.effective(&st, name);
            (old != new).then(|| SettingChange { name: def.name.clone(), old, new, origin: ChangeOrigin::Set })
        };
        Ok(self.publish(change.into_iter().collect()))
    }

    /// Forgets the user's value of `name`: it reads the machine's value or the default again. Returns whether the
    /// effective value changed.
    pub fn reset(&self, name: &str) -> Result<bool, StorageError> {
        let def = self.def_for(name, None)?;
        let layer = self.writable_layer(&def)?;
        let change = {
            let mut st = lock(&self.inner.state);
            let old = self.effective(&st, name);
            let had = st.stored.get_mut(&layer).and_then(|m| m.remove(&def.name)).is_some();
            if had {
                st.dirty.insert((layer, def.name.clone()), None);
            }
            let new = self.effective(&st, name);
            (old != new).then(|| SettingChange { name: def.name.clone(), old, new, origin: ChangeOrigin::Reset })
        };
        Ok(self.publish(change.into_iter().collect()))
    }

    /// [`Settings::reset`] of every writable setting (WinForms' `Reset()`).
    pub fn reset_all(&self) -> Result<(), StorageError> {
        for name in self.names() {
            if !self.is_read_only(&name) {
                self.reset(&name)?;
            }
        }
        Ok(())
    }

    /// Stores what changed since the last save. On failure the changes stay pending (a later save retries) and
    /// the store is untouched (a corrupted file is never overwritten).
    pub fn save(&self) -> Result<(), StorageError> {
        let pending: BTreeMap<(Layer, String), Option<SettingValue>> = lock(&self.inner.state).dirty.clone();
        if pending.is_empty() {
            return Ok(());
        }
        let mut by_layer: BTreeMap<Layer, Vec<(String, Option<SettingValue>)>> = BTreeMap::new();
        for ((layer, name), v) in &pending {
            by_layer.entry(*layer).or_default().push((name.clone(), v.clone()));
        }
        for (layer, changes) in by_layer {
            if let Err(e) = self.inner.backend.write(layer, &changes, self.inner.schema.version) {
                tracing::warn!(target: "kubuno_app_storage", app = %self.inner.app, set = %self.inner.schema.set, layer = layer.name(), "saving settings failed: {e}");
                return Err(e);
            }
            let stamp = self.inner.backend.stamp(layer);
            let mut st = lock(&self.inner.state);
            for (name, v) in changes {
                // Only what was written: a value changed again meanwhile stays pending.
                if st.dirty.get(&(layer, name.clone())) == Some(&v) {
                    st.dirty.remove(&(layer, name));
                }
            }
            st.stamps.insert(layer, stamp);
        }
        Ok(())
    }

    /// Reads everything again from the store, **dropping unsaved changes** (WinForms' `Reload()`), and notifies
    /// what changed.
    pub fn reload(&self) {
        self.load(false, Some(ChangeOrigin::Reload));
    }

    /// Reloads when another process changed the store since this one last read or wrote it (keeping this
    /// process's unsaved changes on top). Returns whether it reloaded. Cheap: one metadata query per layer; a UI
    /// calls it every second or two.
    pub fn refresh_if_changed(&self) -> bool {
        let changed = {
            let st = lock(&self.inner.state);
            Layer::ALL.iter().any(|l| st.stamps.get(l).copied().flatten() != self.inner.backend.stamp(*l))
        };
        if changed {
            self.load(true, Some(ChangeOrigin::External));
        }
        changed
    }

    /// Calls `f` after each change of an effective value (on the thread that made it; keep it short). See
    /// [`Subscription`].
    pub fn subscribe(&self, f: impl Fn(&SettingChange) + Send + Sync + 'static) -> Subscription {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.inner.subscribers).push((id, Arc::new(f)));
        Subscription { inner: Arc::downgrade(&self.inner), id }
    }

    // ── internals ──────────────────────────────────────────────────────────────────────────────

    /// The layer an open schema's values live in.
    fn open_layer(&self) -> Layer {
        if self.inner.schema.open_local {
            Layer::UserLocal
        } else {
            Layer::UserRoaming
        }
    }

    fn effective(&self, st: &State, name: &str) -> Option<SettingValue> {
        let stored = |layer: Layer, n: &str| st.stored.get(&layer).and_then(|m| m.get(n));
        match self.inner.schema.find(name) {
            Some(d) => {
                let read = |layer| stored(layer, &d.name).and_then(|v| v.coerce(d.ty)).filter(|v| d.accept(v).is_ok());
                let v = match d.scope {
                    SettingScope::User => read(d.layer()).or_else(|| read(Layer::Machine)),
                    SettingScope::Application => read(Layer::Machine),
                };
                Some(v.unwrap_or_else(|| d.default.clone()))
            }
            None if self.inner.schema.open => stored(self.open_layer(), name).or_else(|| stored(Layer::Machine, name)).cloned(),
            None => None,
        }
    }

    fn all_effective(&self, st: &State) -> BTreeMap<String, Option<SettingValue>> {
        let mut names: BTreeSet<String> = self.inner.schema.defs.iter().map(|d| d.name.clone()).collect();
        if self.inner.schema.open {
            for m in st.stored.values() {
                names.extend(m.keys().cloned());
            }
        }
        names.into_iter().map(|n| (n.clone(), self.effective(st, &n))).collect()
    }

    /// Reads every layer (upgrading the old ones), optionally keeps the unsaved changes on top, and notifies the
    /// differences with `origin` (none at the first load).
    fn load(&self, keep_dirty: bool, origin: Option<ChangeOrigin>) {
        let mut fresh: BTreeMap<Layer, BTreeMap<String, SettingValue>> = BTreeMap::new();
        let mut stamps = BTreeMap::new();
        let mut errors = BTreeMap::new();
        for layer in Layer::ALL {
            match self.inner.backend.read(layer) {
                Ok(stored) => {
                    let values = self.upgrade(layer, stored.version, stored.values);
                    fresh.insert(layer, values);
                }
                Err(e) => {
                    // Nothing read: the values fall back to their defaults, and the store is left as it is.
                    tracing::warn!(target: "kubuno_app_storage", app = %self.inner.app, set = %self.inner.schema.set, layer = layer.name(), "reading settings failed: {e}");
                    errors.insert(layer, e.to_string());
                }
            }
            stamps.insert(layer, self.inner.backend.stamp(layer));
        }
        let changes = {
            let mut st = lock(&self.inner.state);
            let before = self.all_effective(&st);
            if keep_dirty {
                for ((layer, name), v) in &st.dirty {
                    let m = fresh.entry(*layer).or_default();
                    match v {
                        Some(v) => {
                            m.insert(name.clone(), v.clone());
                        }
                        None => {
                            m.remove(name);
                        }
                    }
                }
            } else {
                st.dirty.clear();
            }
            st.stored = fresh;
            st.stamps = stamps;
            st.errors = errors;
            let after = self.all_effective(&st);
            match origin {
                None => Vec::new(),
                Some(origin) => after
                    .into_iter()
                    .filter(|(n, v)| before.get(n) != Some(v))
                    .map(|(name, new)| SettingChange { old: before.get(&name).cloned().flatten(), name, new, origin })
                    .collect(),
            }
        };
        self.publish(changes);
    }

    /// Upgrades a layer read for `version` (see the module doc); writes the result when the layer is writable.
    fn upgrade(&self, layer: Layer, version: Option<u32>, mut values: BTreeMap<String, SettingValue>) -> BTreeMap<String, SettingValue> {
        let to = self.inner.schema.version;
        let Some(from) = version.filter(|v| *v < to) else { return values };
        let mut changes: BTreeMap<String, Option<SettingValue>> = BTreeMap::new();
        {
            let mut up = Upgrade { from, to, layer, values: &mut values, changes: &mut changes };
            for d in &self.inner.schema.defs {
                for old in &d.previous_names {
                    up.rename(old, &d.name);
                }
            }
            if let Some(f) = &self.inner.options.upgrade {
                f(&mut up);
            }
        }
        let writable = layer != Layer::Machine || self.inner.options.allow_machine_writes;
        if writable {
            let list: Vec<(String, Option<SettingValue>)> = changes.into_iter().collect();
            match self.inner.backend.write(layer, &list, to) {
                Ok(()) => tracing::info!(target: "kubuno_app_storage", app = %self.inner.app, set = %self.inner.schema.set, layer = layer.name(), from, to, "settings upgraded"),
                Err(e) => tracing::warn!(target: "kubuno_app_storage", app = %self.inner.app, set = %self.inner.schema.set, layer = layer.name(), "upgrading settings failed: {e}"),
            }
        }
        values
    }

    /// Bumps the generation and calls the subscribers (outside every lock). Returns whether anything changed.
    fn publish(&self, changes: Vec<SettingChange>) -> bool {
        if changes.is_empty() {
            return false;
        }
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        let subscribers: Vec<Callback> = lock(&self.inner.subscribers).iter().map(|(_, f)| f.clone()).collect();
        for change in &changes {
            for f in &subscribers {
                f(change);
            }
        }
        true
    }
}

/// What the setters of the typed classes call: [`Settings::set`] then [`Settings::save`]. A failure is logged with
/// the setting's name and the error (never the value) and the value stays pending for the next save.
pub fn set_and_save(settings: &Settings, name: &str, value: SettingValue) {
    let result = settings.set(name, value).and_then(|_| settings.save());
    if let Err(e) = result {
        tracing::warn!(target: "kubuno_app_storage", app = %settings.app(), set = %settings.schema().set, setting = name, "the setting was not saved: {e}");
    }
}

// ── The schemas the typed classes register ───────────────────────────────────────────────────────

static SCHEMAS: Mutex<Vec<(AppId, SettingsSchema)>> = Mutex::new(Vec::new());

/// Registers the schema of a typed settings class (`settings!` does it before `main` and on first use) so that a
/// `<Settings Schema="settings">` component of a view finds its declarations. The first registration also makes
/// `app` the process's default app id when none was set.
pub fn register_schema(app: &AppId, schema: SettingsSchema) {
    let mut all = lock(&SCHEMAS);
    if all.is_empty() {
        crate::app::set_default_app_id_if_unset(app.clone());
    }
    match all.iter_mut().find(|(a, s)| a == app && s.set == schema.set) {
        Some(slot) => slot.1 = schema,
        None => all.push((app.clone(), schema)),
    }
}

/// The schema registered for `set` (the first app's when several apps of the process declare one: a component
/// naming its `AppId` picks with [`registered_schema_of`]).
pub fn registered_schema(set: &str) -> Option<(AppId, SettingsSchema)> {
    lock(&SCHEMAS).iter().find(|(_, s)| s.set == set).cloned()
}

/// The schema `app` registered for `set`.
pub fn registered_schema_of(app: &AppId, set: &str) -> Option<SettingsSchema> {
    lock(&SCHEMAS).iter().find(|(a, s)| a == app && s.set == set).map(|(_, s)| s.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingType;

    fn schema(version: u32) -> SettingsSchema {
        SettingsSchema::new("settings", version)
            .with(SettingDef::new("Theme", "System").one_of(&["System", "Light", "Dark"]))
            .with(SettingDef::new("Interval", 5i64).previously("SyncInterval"))
            .with(SettingDef::new("Window", "").local())
            .with(SettingDef::new("Channel", "stable").application())
            .with(SettingDef::new("Recent", Vec::<String>::new()).local())
    }

    fn app() -> AppId {
        AppId::new("unit-app").expect("id")
    }

    /// A memory back-end shared with the test, so it can play "another process".
    #[derive(Debug, Clone, Default)]
    struct Shared(Arc<MemoryBackend>);
    impl SettingsBackend for Shared {
        fn name(&self) -> &'static str {
            "memory"
        }
        fn location(&self, l: Layer) -> String {
            self.0.location(l)
        }
        fn read(&self, l: Layer) -> Result<crate::backend::StoredLayer, StorageError> {
            self.0.read(l)
        }
        fn write(&self, l: Layer, c: &[crate::backend::Change], v: u32) -> Result<(), StorageError> {
            self.0.write(l, c, v)
        }
        fn stamp(&self, l: Layer) -> Option<u64> {
            self.0.stamp(l)
        }
    }

    #[test]
    fn defaults_scopes_and_layers() {
        let mem = Shared::default();
        mem.0.put(Layer::Machine, &[("Channel", "beta".into()), ("Theme", "Dark".into())], Some(1));
        let s = Settings::with_backend(&app(), schema(1), Box::new(mem.clone()), SettingsOptions::default()).expect("open");
        assert_eq!(s.get_as::<String>("Channel").as_deref(), Some("beta"), "application scope reads the machine");
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("Dark"), "an administrator's default");
        assert_eq!(s.get_as::<i64>("Interval"), Some(5), "the schema's default");
        assert!(matches!(s.set("Channel", "x"), Err(StorageError::ReadOnly(_))));
        assert!(matches!(s.set("Theme", "Blue"), Err(StorageError::Setting { .. })));
        assert!(matches!(s.set("Nope", 1i64), Err(StorageError::Setting { .. })));
        assert!(s.set("Theme", "Light").expect("set"));
        assert!(!s.set("Theme", "Light").expect("same value"), "no change, no notification");
        s.set("Window", "10,10,800,600").expect("set");
        s.set("Recent", vec!["a".to_string()]).expect("set");
        assert!(s.is_dirty());
        s.save().expect("save");
        assert!(!s.is_dirty());
        assert_eq!(mem.0.snapshot(Layer::UserRoaming).values.get("Theme"), Some(&SettingValue::from("Light")));
        assert_eq!(mem.0.snapshot(Layer::UserLocal).values.len(), 2, "local settings in the local layer");
        assert_eq!(mem.0.snapshot(Layer::UserRoaming).version, Some(1));
        assert!(s.reset("Theme").expect("reset"));
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("Dark"), "back to the administrator's default");
        s.save().expect("save");
        assert!(!mem.0.snapshot(Layer::UserRoaming).values.contains_key("Theme"));
    }

    #[test]
    fn notifications_and_external_changes() {
        let mem = Shared::default();
        let s = Settings::with_backend(&app(), schema(1), Box::new(mem.clone()), SettingsOptions::default()).expect("open");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sub = {
            let seen = seen.clone();
            s.subscribe(move |c| lock(&seen).push((c.name.clone(), c.origin)))
        };
        let g = s.generation();
        s.set("Interval", 10i64).expect("set");
        assert!(s.generation() > g);
        assert!(!s.refresh_if_changed(), "nothing changed outside");
        // Another instance writes the same store.
        mem.0.put(Layer::UserRoaming, &[("Theme", "Dark".into())], Some(1));
        assert!(s.refresh_if_changed());
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("Dark"));
        assert_eq!(s.get_as::<i64>("Interval"), Some(10), "unsaved changes survive an external refresh");
        s.reload();
        assert_eq!(s.get_as::<i64>("Interval"), Some(5), "reload drops unsaved changes");
        assert_eq!(*lock(&seen), vec![("Interval".to_string(), ChangeOrigin::Set), ("Theme".to_string(), ChangeOrigin::External), ("Interval".to_string(), ChangeOrigin::Reload)]);
        drop(sub);
        s.set("Interval", 11i64).expect("set");
        assert_eq!(lock(&seen).len(), 3, "unsubscribed");
    }

    #[test]
    fn older_layers_are_upgraded_newer_ones_are_not_downgraded() {
        let mem = Shared::default();
        mem.0.put(Layer::UserRoaming, &[("SyncInterval", SettingValue::Int(15)), ("Theme", "light".into()), ("Unknown", true.into())], Some(1));
        let options = SettingsOptions {
            upgrade: Some(Arc::new(|u: &mut Upgrade<'_>| {
                assert_eq!((u.from_version(), u.to_version()), (1, 2));
                if let Some(SettingValue::String(t)) = u.get("Theme").cloned() {
                    u.set("Theme", if t == "light" { "Light" } else { "System" });
                }
            })),
            ..Default::default()
        };
        let s = Settings::with_backend(&app(), schema(2), Box::new(mem.clone()), options).expect("open");
        assert_eq!(s.get_as::<i64>("Interval"), Some(15), "renamed");
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("Light"), "the app's upgrade step ran");
        let stored = mem.0.snapshot(Layer::UserRoaming);
        assert_eq!(stored.version, Some(2));
        assert!(!stored.values.contains_key("SyncInterval"));
        assert_eq!(stored.values.get("Unknown"), Some(&SettingValue::Bool(true)), "unknown names are kept");
        // Version 1 of the app opens the store written by version 2: read as it is, never downgraded.
        let old = Settings::with_backend(&app(), schema(1), Box::new(mem.clone()), SettingsOptions::default()).expect("open");
        old.set("Interval", 20i64).expect("set");
        old.save().expect("save");
        assert_eq!(mem.0.snapshot(Layer::UserRoaming).version, Some(2));
    }

    #[test]
    fn values_of_the_wrong_shape_read_as_defaults() {
        let mem = Shared::default();
        mem.0.put(Layer::UserRoaming, &[("Interval", "soon".into()), ("Theme", "Purple".into())], Some(1));
        let s = Settings::with_backend(&app(), schema(1), Box::new(mem), SettingsOptions::default()).expect("open");
        assert_eq!(s.get_as::<i64>("Interval"), Some(5));
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("System"));
    }

    #[test]
    fn open_schemas_accept_any_name() {
        let s = Settings::with_backend(&app(), SettingsSchema::open("prefs"), Box::new(MemoryBackend::new()), SettingsOptions::default()).expect("open");
        assert_eq!(s.get("Anything"), None);
        s.set("Zoom", 1.25f64).expect("set");
        assert_eq!(s.get("Zoom").map(|v| v.ty()), Some(SettingType::Float));
        assert_eq!(s.names(), vec!["Zoom".to_string()]);
        assert!(s.set("bad name", 1i64).is_err());
    }

    #[test]
    fn shared_instances_are_shared_and_upgraded_from_open() {
        let a = AppId::new("shared-test").expect("id");
        let open = Settings::shared(&a, &SettingsSchema::open("s1"), BackendKind::Memory).expect("open");
        let real = SettingsSchema::new("s1", 1).with(SettingDef::new("X", 3i64));
        let typed = Settings::shared(&a, &real, BackendKind::Memory).expect("typed");
        assert!(!Arc::ptr_eq(&open.inner, &typed.inner), "the open stand-in is replaced");
        let again = Settings::shared(&a, &SettingsSchema::open("s1"), BackendKind::Memory).expect("again");
        assert!(Arc::ptr_eq(&again.inner, &typed.inner));
        assert_eq!(again.get_as::<i64>("X"), Some(3));
    }
}
