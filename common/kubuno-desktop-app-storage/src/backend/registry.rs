//! [`RegistryBackend`] (Windows): settings as Registry values.
//!
//! ```text
//! HKCU\Software\Kubuno\Apps\<app>\<set>                                     roaming user settings
//! HKCU\Software\Classes\Local Settings\Software\Kubuno\Apps\<app>\<set>     local user settings (never roams)
//! HKLM\Software\Kubuno\Apps\<app>\<set>  (64-bit view)                      machine settings (read-only for apps)
//! ```
//!
//! Types: `Bool` → `REG_DWORD` 0/1, `Int` → `REG_QWORD`, `Float` → `REG_SZ` (invariant text), `String` → `REG_SZ`,
//! `StringList` → `REG_MULTI_SZ` (no empty item), the schema version → `REG_DWORD` `$version`. Values of other
//! types or names are left alone. Every access goes through a [`RegistryRoot`]: in a sandbox they are redirected.

use super::{Change, SettingsBackend, StoredLayer};
use crate::account::AccountKey;
use crate::app::AppId;
use crate::registry::{Access, Hive, RegValue, RegistryRoot, RegistryView, LOCAL_SETTINGS};
use crate::settings::{Layer, SettingValue};
use crate::StorageError;

/// The value holding the schema version.
const VERSION_VALUE: &str = "$version";

/// See the module doc.
#[derive(Debug, Clone)]
pub struct RegistryBackend {
    app: AppId,
    set: String,
    root: RegistryRoot,
    account: Option<AccountKey>,
}

impl RegistryBackend {
    /// The real Registry (redirected in a sandbox).
    pub fn new(app: &AppId, set: &str) -> Self {
        Self::with_root(app, set, RegistryRoot::current())
    }

    /// Below another root (tests: [`RegistryRoot::under`]).
    pub fn with_root(app: &AppId, set: &str, root: RegistryRoot) -> Self {
        Self { app: app.clone(), set: set.to_string(), root, account: None }
    }

    /// Account-scoped (decision Q6): the user layers below `HKCU\Software\Kubuno\Accounts\<key>\Apps\<app>\<set>`
    /// (the local one below `Local Settings`), the machine layer unchanged.
    pub fn with_account(mut self, account: Option<AccountKey>) -> Self {
        self.account = account;
        self
    }

    /// The hive, path and view of `layer`.
    pub fn key(&self, layer: Layer) -> (Hive, String, RegistryView) {
        let machine = format!(r"Software\Kubuno\Apps\{}\{}", self.app, self.set);
        let user = match &self.account {
            Some(a) => format!(r"Software\Kubuno\Accounts\{a}\Apps\{}\{}", self.app, self.set),
            None => machine.clone(),
        };
        match layer {
            Layer::UserRoaming => (Hive::CurrentUser, user, RegistryView::Default),
            Layer::UserLocal => (Hive::CurrentUser, format!(r"{LOCAL_SETTINGS}\{user}"), RegistryView::Default),
            // One machine key for the 32- and the 64-bit builds of an app.
            Layer::Machine => (Hive::LocalMachine, machine, RegistryView::Registry64),
        }
    }

    fn encode(name: &str, v: &SettingValue) -> Result<RegValue, StorageError> {
        Ok(match v {
            SettingValue::Bool(b) => RegValue::DWord(u32::from(*b)),
            SettingValue::Int(i) => RegValue::QWord(*i as u64),
            SettingValue::Float(_) | SettingValue::String(_) => RegValue::String(v.to_text()),
            SettingValue::StringList(l) => {
                if l.iter().any(String::is_empty) {
                    return Err(StorageError::Setting { name: name.to_string(), message: "the Registry cannot store an empty list item".into() });
                }
                RegValue::MultiString(l.clone())
            }
        })
    }

    fn decode(v: RegValue) -> Option<SettingValue> {
        Some(match v {
            RegValue::String(s) | RegValue::ExpandString(s) => SettingValue::String(s),
            RegValue::MultiString(l) => SettingValue::StringList(l),
            RegValue::DWord(d) => SettingValue::Int(i64::from(d)),
            RegValue::QWord(q) => SettingValue::Int(q as i64),
            RegValue::Binary(_) => return None,
        })
    }
}

impl SettingsBackend for RegistryBackend {
    fn name(&self) -> &'static str {
        "registry"
    }

    fn location(&self, layer: Layer) -> String {
        let (h, p, _) = self.key(layer);
        self.root.display(h, &p)
    }

    fn read(&self, layer: Layer) -> Result<StoredLayer, StorageError> {
        let (h, p, view) = self.key(layer);
        let Some(key) = self.root.open(h, &p, view, Access::Read)? else { return Ok(StoredLayer::default()) };
        let mut out = StoredLayer::default();
        for name in key.value_names()? {
            let Some(v) = key.get_value(&name)? else { continue };
            if name == VERSION_VALUE {
                out.version = match v {
                    RegValue::DWord(d) => Some(d),
                    _ => None,
                };
                continue;
            }
            if let Some(v) = Self::decode(v) {
                out.values.insert(name, v);
            }
        }
        if out.version.is_none() && !out.values.is_empty() {
            out.version = Some(0);
        }
        Ok(out)
    }

    fn write(&self, layer: Layer, changes: &[Change], version: u32) -> Result<(), StorageError> {
        let (h, p, view) = self.key(layer);
        // Encode everything first: a refused value writes nothing.
        let encoded = changes.iter().map(|(k, v)| Ok((k, v.as_ref().map(|v| Self::encode(k, v)).transpose()?))).collect::<Result<Vec<_>, StorageError>>()?;
        let key = self.root.create(h, &p, view)?;
        for (k, v) in encoded {
            match v {
                Some(v) => key.set_value(k, &v)?,
                None => {
                    key.delete_value(k)?;
                }
            }
        }
        let stored = match key.get_value(VERSION_VALUE)? {
            Some(RegValue::DWord(d)) => d,
            _ => 0,
        };
        key.set_value(VERSION_VALUE, &RegValue::DWord(stored.max(version)))
    }

    fn stamp(&self, layer: Layer) -> Option<u64> {
        let (h, p, view) = self.key(layer);
        self.root.open(h, &p, view, Access::Read).ok().flatten().and_then(|k| k.last_write_time().ok())
    }
}
