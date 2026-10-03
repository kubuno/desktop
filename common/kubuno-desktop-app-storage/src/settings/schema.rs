//! What an app declares about its settings: their names, types, scopes, defaults and history (WinForms'
//! `Settings.settings`; Kubuno writes it as a `.kbsettings` file read by `kubuno_desktop::settings!`, or in code).

use super::value::{SettingType, SettingValue};
use crate::StorageError;

/// The largest value of a setting (a string, or a list's items together), in bytes.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// The most items of a `StringList`.
pub const MAX_LIST_ITEMS: usize = 1024;
/// The largest stored set (a settings file), in bytes: settings are not a database.
pub const MAX_SET_BYTES: usize = 1024 * 1024;
/// The longest setting name.
pub const MAX_NAME: usize = 128;

/// Who a setting belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingScope {
    /// Each user has their own value, which the app reads and writes (WinForms' user scope).
    #[default]
    User,
    /// One value for the machine, deployed by an administrator; read-only for the app (WinForms' application
    /// scope).
    Application,
}

impl SettingScope {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "user" => Some(SettingScope::User),
            "application" | "app" | "machine" => Some(SettingScope::Application),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SettingScope::User => "User",
            SettingScope::Application => "Application",
        }
    }
}

/// Where a value is stored (a back-end keeps each layer apart).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// The user's settings that follow them between machines (Windows roaming profile: `%APPDATA%`, `HKCU`).
    UserRoaming,
    /// The user's settings of this machine only (window positions, local paths: `%LOCALAPPDATA%`,
    /// `HKCU\Software\Classes\Local Settings`).
    UserLocal,
    /// The machine's values (application scope, and administrators' defaults of user settings).
    Machine,
}

impl Layer {
    pub const ALL: [Layer; 3] = [Layer::UserRoaming, Layer::UserLocal, Layer::Machine];

    pub fn name(self) -> &'static str {
        match self {
            Layer::UserRoaming => "user (roaming)",
            Layer::UserLocal => "user (local)",
            Layer::Machine => "machine",
        }
    }
}

/// One declared setting.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDef {
    /// `PascalCase` by convention (`Theme`, `SyncIntervalMinutes`); the typed accessor is its snake case.
    pub name: String,
    pub ty: SettingType,
    pub scope: SettingScope,
    /// User scope: whether the value roams with the user's profile (default) or stays on this machine.
    pub roaming: bool,
    pub default: SettingValue,
    pub description: String,
    /// Names this setting had in earlier versions of the schema: a stored value under one of them is moved to
    /// the current name by the upgrade.
    pub previous_names: Vec<String>,
    /// For a `String`: the accepted values (an enumeration: `System`, `Light`, `Dark`); empty = any text.
    pub values: Vec<String>,
}

impl SettingDef {
    /// A user-scoped, roaming setting whose type is the default's.
    pub fn new(name: &str, default: impl Into<SettingValue>) -> Self {
        let default = default.into();
        Self {
            name: name.to_string(),
            ty: default.ty(),
            scope: SettingScope::User,
            roaming: true,
            default,
            description: String::new(),
            previous_names: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Application scope (read-only for the app).
    pub fn application(mut self) -> Self {
        self.scope = SettingScope::Application;
        self
    }

    /// Kept on this machine only (not roaming).
    pub fn local(mut self) -> Self {
        self.roaming = false;
        self
    }

    pub fn describe(mut self, text: &str) -> Self {
        self.description = text.to_string();
        self
    }

    /// An earlier name (see [`SettingDef::previous_names`]).
    pub fn previously(mut self, name: &str) -> Self {
        self.previous_names.push(name.to_string());
        self
    }

    /// The accepted values of a `String` setting.
    pub fn one_of(mut self, values: &[&str]) -> Self {
        self.values = values.iter().map(|v| v.to_string()).collect();
        self
    }

    /// The layer the user's value of this setting lives in; the machine layer for application scope.
    pub fn layer(&self) -> Layer {
        match (self.scope, self.roaming) {
            (SettingScope::Application, _) => Layer::Machine,
            (SettingScope::User, true) => Layer::UserRoaming,
            (SettingScope::User, false) => Layer::UserLocal,
        }
    }

    /// `value` converted to this setting's type and checked against its limits and accepted values.
    pub fn accept(&self, value: &SettingValue) -> Result<SettingValue, StorageError> {
        let v = value.coerce(self.ty).ok_or_else(|| StorageError::Setting { name: self.name.clone(), message: format!("a {} value is expected, not a {}", self.ty, value.ty()) })?;
        check_limits(&self.name, &v)?;
        if let (SettingValue::String(s), false) = (&v, self.values.is_empty()) {
            if !self.values.iter().any(|a| a == s) {
                return Err(StorageError::Setting { name: self.name.clone(), message: format!("the value is not one of {}", self.values.join(", ")) });
            }
        }
        Ok(v)
    }
}

/// The size limits of a value (`docs/STORAGE-COMPONENTS.md` §6.3).
pub fn check_limits(name: &str, v: &SettingValue) -> Result<(), StorageError> {
    if v.byte_len() > MAX_VALUE_BYTES {
        return Err(StorageError::TooLarge { name: name.to_string(), len: v.byte_len(), max: MAX_VALUE_BYTES });
    }
    if let SettingValue::StringList(l) = v {
        if l.len() > MAX_LIST_ITEMS {
            return Err(StorageError::TooLarge { name: name.to_string(), len: l.len(), max: MAX_LIST_ITEMS });
        }
    }
    Ok(())
}

/// Whether `name` is a valid setting name: a letter or `_`, then letters, digits and `_`, at most
/// [`MAX_NAME`] characters (it is a JSON key, a Registry value name and, in snake case, a Rust identifier).
pub fn valid_setting_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `set` is a valid set name (the stem of a `.kbsettings` file, lower case: `settings`, `window.state`):
/// it becomes a file name and a Registry key.
pub fn valid_set_name(set: &str) -> bool {
    !set.is_empty()
        && set.len() <= 64
        && set.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && !set.ends_with('.')
        && set.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// The declared settings of one set.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSchema {
    /// The set's name (`settings` for `settings.kbsettings`): one stored file / Registry key per set.
    pub set: String,
    /// Bumped when settings are renamed, retyped or removed; the stored version drives the upgrade.
    pub version: u32,
    pub defs: Vec<SettingDef>,
    /// An open schema accepts any name (a `<Settings>` component whose `.kbsettings` is unknown at run time):
    /// values keep their natural type, nothing has a default.
    pub open: bool,
    /// An open schema's values live in the local layer (this machine only) instead of the roaming one.
    pub open_local: bool,
    /// The values belong to the current account (`crate::account`, decision Q6) rather than to the app: they live
    /// below `<user_data_dir>/accounts/<key>/<app>/`.
    pub account_scoped: bool,
}

impl SettingsSchema {
    pub fn new(set: &str, version: u32) -> Self {
        Self { set: set.to_string(), version, defs: Vec::new(), open: false, open_local: false, account_scoped: false }
    }

    /// A schema that declares nothing and accepts every name (see [`SettingsSchema::open`]).
    pub fn open(set: &str) -> Self {
        Self { set: set.to_string(), version: 1, defs: Vec::new(), open: true, open_local: false, account_scoped: false }
    }

    /// An open schema whose values stay on this machine (the local layer): what an app's existing settings struct
    /// is stored as through [`crate::settings::serde_bridge`].
    pub fn open_local(set: &str) -> Self {
        Self { open_local: true, ..Self::open(set) }
    }

    /// Makes the values belong to the current account (builder form; see [`SettingsSchema::account_scoped`]).
    pub fn per_account(mut self) -> Self {
        self.account_scoped = true;
        self
    }

    /// Adds a setting (builder form).
    pub fn with(mut self, def: SettingDef) -> Self {
        self.defs.push(def);
        self
    }

    pub fn find(&self, name: &str) -> Option<&SettingDef> {
        self.defs.iter().find(|d| d.name == name)
    }

    /// Checks the schema: valid set and setting names, no two names equal ignoring case (Registry value names are
    /// case-insensitive), defaults of the declared type and within the limits, accepted values only on strings.
    pub fn validate(&self) -> Result<(), StorageError> {
        if !valid_set_name(&self.set) {
            return Err(StorageError::InvalidName(format!("settings set '{}' (lower-case letters, digits, '.', '-', '_')", self.set)));
        }
        if self.version == 0 {
            return Err(StorageError::InvalidName(format!("settings set '{}': the version starts at 1", self.set)));
        }
        let mut seen: Vec<String> = Vec::new();
        for d in &self.defs {
            if !valid_setting_name(&d.name) {
                return Err(StorageError::InvalidName(format!("setting '{}' (a letter or '_', then letters, digits and '_')", d.name)));
            }
            for n in std::iter::once(&d.name).chain(&d.previous_names) {
                let key = n.to_ascii_lowercase();
                if seen.contains(&key) {
                    return Err(StorageError::InvalidName(format!("setting '{n}' is declared twice (names are compared ignoring case)")));
                }
                seen.push(key);
            }
            if d.default.ty() != d.ty {
                return Err(StorageError::Setting { name: d.name.clone(), message: format!("the default is a {}, the setting a {}", d.default.ty(), d.ty) });
            }
            if !d.values.is_empty() && d.ty != SettingType::String {
                return Err(StorageError::Setting { name: d.name.clone(), message: "accepted values are only for String settings".into() });
            }
            d.accept(&d.default)?;
        }
        Ok(())
    }
}

/// Whether a setting's name suggests a secret (`ApiToken`, `SmtpPassword`): such values belong in a
/// `SecretStore`, never in settings (plain files, the Registry). The language server and the macro warn.
pub fn looks_like_secret(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "apikey", "api_key", "credential", "privatekey", "private_key"].iter().any(|w| n.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_validated() {
        let ok = SettingsSchema::new("settings", 1)
            .with(SettingDef::new("Theme", "System").one_of(&["System", "Light", "Dark"]))
            .with(SettingDef::new("Interval", 5i64).previously("SyncInterval"))
            .with(SettingDef::new("Channel", "stable").application());
        assert!(ok.validate().is_ok());
        assert_eq!(ok.find("Channel").map(SettingDef::layer), Some(Layer::Machine));
        assert!(SettingsSchema::new("Settings", 1).validate().is_err(), "upper-case set");
        assert!(SettingsSchema::new("s", 1).with(SettingDef::new("a", 1i64)).with(SettingDef::new("A", 2i64)).validate().is_err());
        assert!(SettingsSchema::new("s", 1).with(SettingDef::new("Theme", "Blue").one_of(&["Light"])).validate().is_err());
        assert!(SettingsSchema::new("s", 1).with(SettingDef::new("1x", true)).validate().is_err());
        assert!(SettingsSchema::new("s", 1).with(SettingDef::new("Big", "x".repeat(MAX_VALUE_BYTES + 1))).validate().is_err());
    }

    #[test]
    fn values_are_accepted_by_type_and_choice() {
        let d = SettingDef::new("Theme", "System").one_of(&["System", "Dark"]);
        assert!(d.accept(&"Dark".into()).is_ok());
        assert!(d.accept(&"Blue".into()).is_err());
        let n = SettingDef::new("Zoom", 1.0f64);
        assert_eq!(n.accept(&SettingValue::Int(2)).ok(), Some(SettingValue::Float(2.0)));
        assert!(n.accept(&SettingValue::Bool(true)).is_err());
    }

    #[test]
    fn secret_like_names() {
        assert!(looks_like_secret("SmtpPassword"));
        assert!(looks_like_secret("api_key"));
        assert!(!looks_like_secret("Theme"));
    }
}
