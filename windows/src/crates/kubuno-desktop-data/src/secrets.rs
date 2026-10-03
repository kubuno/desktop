//! Secrets: where connection strings and passwords come from (`vskubuno/docs/DATA.md` §3). Never
//! from the source or the `.kbview`: a connection names a key (`ConnectionStringName="Northwind"`),
//! and a [`SecretResolver`] looks the key `ConnectionStrings:Northwind` up in, first hit wins:
//!
//! 1. the environment: `ConnectionStrings__Northwind` (CI, services — the .NET convention);
//! 2. the app's secrets of the unified naming scheme (vskubuno `docs/STORAGE-COMPONENTS.md`, decision Q4): the OS
//!    credential store, target `Kubuno/app.<UserSecretsId>/ConnectionStrings.Northwind` ([`AppSecretsSource`]);
//! 3. the older Windows Credential Manager credential `Kubuno:<UserSecretsId>:ConnectionStrings:Northwind` (its
//!    blob, UTF-8), copied into the unified scheme when found ([`MigratingSource`]);
//! 4. the user secrets store, for development: `%APPDATA%\Kubuno\UserSecrets\<UserSecretsId>\secrets.json`,
//!    a JSON object (`{"ConnectionStrings:Northwind": "…"}`; nested objects are flattened with `:`).
//!
//! The id is the application's `[package.metadata.kubuno] user-secrets-id = "…"` (read at compile
//! time by [`user_secrets_id!`](crate::user_secrets_id)), registered once with [`set_user_secrets_id`].
//! A connection string can keep its password out of itself with `{secret:Key}` placeholders
//! ([`SecretResolver::expand`]). Errors name keys, never values.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::error::DataError;

/// A place secrets can be read from.
pub trait SecretSource: Send + Sync {
    /// For logs (`"environment"`, `"credential manager"`, `"user secrets"`).
    fn name(&self) -> &'static str;
    /// The secret stored under `key`, if this source has one.
    fn get(&self, key: &str) -> Result<Option<String>, DataError>;
}

/// Environment variables: the key with `:` replaced by `__` (`ConnectionStrings__Northwind`).
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvironmentSecrets;

impl EnvironmentSecrets {
    /// The variable name of `key`.
    pub fn variable(key: &str) -> String {
        key.replace(':', "__")
    }
}

impl SecretSource for EnvironmentSecrets {
    fn name(&self) -> &'static str {
        "environment"
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        Ok(std::env::var(Self::variable(key)).ok().filter(|v| !v.is_empty()))
    }
}

/// Whether `id` is usable as a user secrets id (it becomes a folder name and part of a credential
/// target): letters, digits, `-`, `_`, `.`; 1 to 128 characters; not only dots.
pub fn is_valid_user_secrets_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && id.chars().any(|c| c != '.')
}

/// The development secrets store (`secrets.json`, see the module doc).
#[derive(Debug, Clone)]
pub struct UserSecrets {
    path: PathBuf,
}

impl UserSecrets {
    /// The store of the application `id`: `%APPDATA%\Kubuno\UserSecrets\<id>\secrets.json`.
    pub fn for_id(id: &str) -> Result<Self, DataError> {
        if !is_valid_user_secrets_id(id) {
            return Err(DataError::Validation("the user secrets id may only contain letters, digits, '-', '_' and '.'".to_string()));
        }
        let appdata = std::env::var_os("APPDATA").ok_or_else(|| DataError::Config("APPDATA is not set: no user secrets store".to_string()))?;
        Ok(Self { path: PathBuf::from(appdata).join("Kubuno").join("UserSecrets").join(id).join("secrets.json") })
    }

    /// A store at an explicit path (tests, tools).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<serde_json::Map<String, serde_json::Value>, DataError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(serde_json::Map::new()),
            Err(e) => return Err(crate::error::logged("user secrets", DataError::Secret(format!("cannot read the user secrets store: {e}")))),
        };
        match serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) {
            Ok(serde_json::Value::Object(map)) => {
                let mut flat = serde_json::Map::new();
                flatten("", map, &mut flat);
                Ok(flat)
            }
            Ok(_) => Err(DataError::Secret("the user secrets store is not a JSON object".to_string())),
            // The parser's message may quote the file's content: only its position is kept.
            Err(e) => Err(crate::error::logged("user secrets", DataError::Secret(format!("the user secrets store is not valid JSON (line {}, column {})", e.line(), e.column())))),
        }
    }

    /// Stores `value` under `key` (creating the store), what `dotnet user-secrets set` does.
    pub fn set(&self, key: &str, value: &str) -> Result<(), DataError> {
        let mut map = self.read()?;
        map.insert(key.to_string(), serde_json::Value::String(value.to_string()));
        self.write(map)
    }

    /// Removes `key`; `Ok(false)` when it was not there.
    pub fn remove(&self, key: &str) -> Result<bool, DataError> {
        let mut map = self.read()?;
        let had = map.remove(key).is_some();
        if had {
            self.write(map)?;
        }
        Ok(had)
    }

    fn write(&self, map: serde_json::Map<String, serde_json::Value>) -> Result<(), DataError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| crate::error::logged("user secrets", DataError::Secret(format!("cannot create the user secrets folder: {e}"))))?;
        }
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(map)).map_err(|e| DataError::Secret(format!("cannot serialize the user secrets: {e}")))?;
        std::fs::write(&self.path, text).map_err(|e| crate::error::logged("user secrets", DataError::Secret(format!("cannot write the user secrets store: {e}"))))
    }
}

fn flatten(prefix: &str, map: serde_json::Map<String, serde_json::Value>, out: &mut serde_json::Map<String, serde_json::Value>) {
    for (k, v) in map {
        let key = if prefix.is_empty() { k } else { format!("{prefix}:{k}") };
        match v {
            serde_json::Value::Object(inner) => flatten(&key, inner, out),
            other => {
                out.insert(key, other);
            }
        }
    }
}

impl SecretSource for UserSecrets {
    fn name(&self) -> &'static str {
        "user secrets"
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        Ok(match self.read()?.remove(key) {
            Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(serde_json::Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        })
    }
}

/// The Windows Credential Manager (generic credentials named `Kubuno:<id>:<key>`).
#[cfg(all(windows, feature = "credential-manager"))]
#[derive(Debug, Clone)]
pub struct CredentialManager {
    prefix: String,
}

#[cfg(all(windows, feature = "credential-manager"))]
impl CredentialManager {
    /// The credentials of the application `id`.
    pub fn for_id(id: &str) -> Result<Self, DataError> {
        if !is_valid_user_secrets_id(id) {
            return Err(DataError::Validation("the user secrets id may only contain letters, digits, '-', '_' and '.'".to_string()));
        }
        Ok(Self { prefix: format!("Kubuno:{id}:") })
    }

    /// The credential's target name for `key`.
    pub fn target(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Stores `secret` under `key` (local machine persistence, the current user only).
    pub fn set(&self, key: &str, secret: &str) -> Result<(), DataError> {
        use windows_sys::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};
        let mut target = Self::wide(&self.target(key));
        let mut user = Self::wide("kubuno");
        let blob = secret.as_bytes();
        let size = u32::try_from(blob.len()).map_err(|_| DataError::Validation("the secret is too large".to_string()))?;
        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: windows_sys::Win32::Foundation::FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: size,
            CredentialBlob: blob.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `cred` points into a buffer that outlives the call; CredWriteW
        // copies what it keeps.
        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            let e = std::io::Error::last_os_error();
            return Err(crate::error::logged("credential manager", DataError::Secret(format!("cannot store the credential for `{key}`: {e}"))));
        }
        Ok(())
    }

    /// Deletes the credential of `key`; `Ok(false)` when there was none.
    pub fn remove(&self, key: &str) -> Result<bool, DataError> {
        use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
        let target = Self::wide(&self.target(key));
        // SAFETY: `target` is a NUL-terminated UTF-16 string alive for the call.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let e = std::io::Error::last_os_error();
            // ERROR_NOT_FOUND (1168).
            if e.raw_os_error() == Some(1168) {
                return Ok(false);
            }
            return Err(crate::error::logged("credential manager", DataError::Secret(format!("cannot delete the credential for `{key}`: {e}"))));
        }
        Ok(true)
    }
}

#[cfg(all(windows, feature = "credential-manager"))]
impl SecretSource for CredentialManager {
    fn name(&self) -> &'static str {
        "credential manager"
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        use windows_sys::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
        let target = Self::wide(&self.target(key));
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is NUL-terminated and alive for the call; `cred` receives a buffer owned by
        // the system, freed with CredFree below.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
        if ok == 0 || cred.is_null() {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(1168) {
                return Ok(None);
            }
            return Err(crate::error::logged("credential manager", DataError::Secret(format!("cannot read the credential for `{key}`: {e}"))));
        }
        // SAFETY: CredReadW succeeded: `cred` points to a valid CREDENTIALW whose blob holds
        // `CredentialBlobSize` bytes; both stay valid until CredFree.
        let secret = unsafe {
            let c = &*cred;
            let bytes = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(cred.cast());
            bytes
        };
        match String::from_utf8(secret) {
            Ok(s) if !s.is_empty() => Ok(Some(s)),
            Ok(_) => Ok(None),
            Err(_) => Err(DataError::Secret(format!("the credential for `{key}` is not UTF-8 text"))),
        }
    }
}

/// The app's secrets of the **unified naming scheme** (vskubuno `docs/STORAGE-COMPONENTS.md`, decision Q4): the OS
/// credential store of `kubuno-desktop-app-storage` (`kubuno-desktop-secrets`: Windows Credential Manager, macOS Keychain, Secret
/// Service), target `Kubuno/app.<id>/<key>` with the key's `:` written `.` (`ConnectionStrings.Northwind`), the same
/// scheme as every Kubuno desktop secret (`Kubuno/<scope>/<item>`), sandbox-aware. Read before the older
/// `Kubuno:<id>:<key>` credentials, which [`MigratingSource`] copies here when it finds one.
#[derive(Debug, Clone)]
pub struct AppSecretsSource {
    secrets: kubuno_desktop_app_storage::AppSecrets,
}

impl AppSecretsSource {
    /// The OS credential store, for the application `id` (its user secrets id, as an app id: lower case).
    pub fn for_id(id: &str) -> Result<Self, DataError> {
        let app = kubuno_desktop_app_storage::AppId::from_name(id).ok_or_else(|| DataError::Validation(format!("`{id}` cannot name the app's secrets")))?;
        let secrets = kubuno_desktop_app_storage::AppSecrets::open(&app).map_err(|e| DataError::Secret(e.to_string()))?;
        Ok(Self { secrets })
    }

    /// Over the app secrets of the caller (tests: an in-memory store).
    pub fn with(secrets: kubuno_desktop_app_storage::AppSecrets) -> Self {
        Self { secrets }
    }

    /// The secret name of `key` (`ConnectionStrings:Northwind` → `ConnectionStrings.Northwind`).
    pub fn name_of(key: &str) -> String {
        key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '.' }).collect()
    }

    /// Stores `value` under `key`.
    pub fn set(&self, key: &str, value: &str) -> Result<(), DataError> {
        self.secrets.set_str(&Self::name_of(key), value).map_err(|e| DataError::Secret(e.to_string()))
    }

    /// Removes `key`; `Ok(false)` when there was none.
    pub fn remove(&self, key: &str) -> Result<bool, DataError> {
        self.secrets.delete(&Self::name_of(key)).map_err(|e| DataError::Secret(e.to_string()))
    }
}

impl SecretSource for AppSecretsSource {
    fn name(&self) -> &'static str {
        "app secrets"
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        let secret = self.secrets.get(&Self::name_of(key)).map_err(|e| DataError::Secret(e.to_string()))?;
        secret
            .map(|s| s.expose_str().map(str::to_string).map_err(|_| DataError::Secret(format!("the secret `{key}` is not UTF-8 text"))))
            .transpose()
            .map(|v| v.filter(|s| !s.is_empty()))
    }
}

/// An older source whose hits are copied into the unified scheme (decision Q4): the `Kubuno:<id>:<key>` credentials
/// are read as before and, when one is found, stored again as an app secret (the old credential is kept for one
/// release, so an older build of the app still finds it). The copy is logged with the key, never the value.
pub struct MigratingSource<S: SecretSource> {
    legacy: S,
    target: AppSecretsSource,
}

impl<S: SecretSource> MigratingSource<S> {
    pub fn new(legacy: S, target: AppSecretsSource) -> Self {
        Self { legacy, target }
    }
}

impl<S: SecretSource> SecretSource for MigratingSource<S> {
    fn name(&self) -> &'static str {
        self.legacy.name()
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        let found = self.legacy.get(key)?;
        if let Some(v) = &found {
            match self.target.set(key, v) {
                Ok(()) => tracing::info!(target: "kubuno_desktop_data", key, from = self.legacy.name(), "secret copied to the unified naming scheme (Kubuno/app.<id>/…)"),
                Err(e) => tracing::warn!(target: "kubuno_desktop_data", key, error = %e, "the secret could not be copied to the unified naming scheme"),
            }
        }
        Ok(found)
    }
}

static USER_SECRETS_ID: Mutex<Option<String>> = Mutex::new(None);

/// Registers the application's user secrets id (`kubuno_desktop_data::set_user_secrets_id(kubuno_desktop_data::user_secrets_id!())`
/// at the start of `main`). Every connection resolved afterwards uses it.
pub fn set_user_secrets_id(id: Option<&str>) {
    *USER_SECRETS_ID.lock().unwrap_or_else(PoisonError::into_inner) = id.filter(|i| is_valid_user_secrets_id(i)).map(str::to_string);
}

/// The id registered with [`set_user_secrets_id`].
pub fn user_secrets_id() -> Option<String> {
    USER_SECRETS_ID.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Reads `user-secrets-id` from the `[package.metadata.kubuno]` table of a `Cargo.toml` text (what
/// the [`user_secrets_id!`](crate::user_secrets_id) macro passes, embedded at compile time).
pub fn user_secrets_id_from_manifest(manifest: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_table = line.trim_start_matches('[').trim_end_matches(']').trim() == "package.metadata.kubuno";
            continue;
        }
        if !in_table {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if matches!(k.trim(), "user-secrets-id" | "user_secrets_id") {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                return is_valid_user_secrets_id(v).then(|| v.to_string());
            }
        }
    }
    None
}

/// The application's user secrets id, from its own `Cargo.toml` (`[package.metadata.kubuno]
/// user-secrets-id = "…"`), read at compile time. `Option<String>`.
#[macro_export]
macro_rules! user_secrets_id {
    () => {
        $crate::secrets::user_secrets_id_from_manifest(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")))
    };
}

/// A chain of [`SecretSource`]s, first hit wins.
pub struct SecretResolver {
    sources: Vec<Box<dyn SecretSource>>,
}

impl std::fmt::Debug for SecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.sources.iter().map(|s| s.name())).finish()
    }
}

impl SecretResolver {
    /// An empty chain (add sources with [`Self::with`]).
    pub fn empty() -> Self {
        Self { sources: Vec::new() }
    }

    /// The default chain of the module doc for the application `id` (the environment only when
    /// there is no id).
    pub fn default_for(id: Option<&str>) -> Self {
        let mut r = Self::empty().with(EnvironmentSecrets);
        if let Some(id) = id {
            // The unified scheme first (decision Q4), then the older credentials, copied into it when found.
            let unified = AppSecretsSource::for_id(id);
            match &unified {
                Ok(s) => r = r.with(s.clone()),
                Err(e) => tracing::warn!(target: "kubuno_desktop_data", error = %e, "the app's secret store is not available"),
            }
            #[cfg(all(windows, feature = "credential-manager"))]
            if let Ok(cm) = CredentialManager::for_id(id) {
                match unified {
                    Ok(target) => r = r.with(MigratingSource::new(cm, target)),
                    Err(_) => r = r.with(cm),
                }
            }
            if let Ok(us) = UserSecrets::for_id(id) {
                r = r.with(us);
            }
        }
        r
    }

    /// The default chain for the registered id ([`set_user_secrets_id`]).
    pub fn default_chain() -> Self {
        Self::default_for(user_secrets_id().as_deref())
    }

    pub fn with(mut self, source: impl SecretSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// The secret stored under `key` in the first source that has it.
    pub fn resolve(&self, key: &str) -> Result<String, DataError> {
        for source in &self.sources {
            match source.get(key) {
                Ok(Some(v)) => {
                    tracing::debug!(target: "kubuno_desktop_data", key, source = source.name(), "secret resolved");
                    return Ok(v);
                }
                Ok(None) => {}
                // A broken source does not hide the next one; the error is already logged.
                Err(e) => tracing::warn!(target: "kubuno_desktop_data", key, source = source.name(), error = %e, "secret source failed"),
            }
        }
        let tried: Vec<&str> = self.sources.iter().map(|s| s.name()).collect();
        Err(crate::error::logged("secrets", DataError::Secret(format!("no secret named `{key}` (looked in: {})", tried.join(", ")))))
    }

    /// Replaces every `{secret:Key}` placeholder of `text` with the secret `Key`.
    pub fn expand(&self, text: &str) -> Result<String, DataError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("{secret:") {
            out.push_str(&rest[..start]);
            let after = &rest[start + "{secret:".len()..];
            let end = after.find('}').ok_or_else(|| DataError::Secret("a `{secret:` placeholder is not closed".to_string()))?;
            let key = after[..end].trim();
            if key.is_empty() {
                return Err(DataError::Secret("a `{secret:}` placeholder has no key".to_string()));
            }
            out.push_str(&self.resolve(key)?);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> UserSecrets {
        let dir = std::env::temp_dir().join(format!("kubuno-data-secrets-{}-{:?}", std::process::id(), std::thread::current().id()));
        UserSecrets::at(dir.join("secrets.json"))
    }

    #[test]
    fn user_secrets_set_get_remove_and_nested_keys() {
        let store = temp_store();
        store.set("ConnectionStrings:Main", "sqlite::memory:").expect("set");
        assert_eq!(store.get("ConnectionStrings:Main").expect("get"), Some("sqlite::memory:".to_string()));
        std::fs::write(store.path(), r#"{"ConnectionStrings": {"Nested": "x"}, "Port": 5432}"#).expect("write");
        assert_eq!(store.get("ConnectionStrings:Nested").expect("get"), Some("x".to_string()));
        assert_eq!(store.get("Port").expect("get"), Some("5432".to_string()));
        assert!(store.remove("Port").expect("remove"));
        assert_eq!(store.get("Port").expect("get"), None);
        std::fs::write(store.path(), "{ not json").expect("write");
        let err = store.get("x").expect_err("invalid");
        assert!(!err.to_string().contains("not json"));
        let _ = std::fs::remove_dir_all(store.path().parent().expect("dir"));
    }

    #[test]
    fn resolver_chain_and_placeholders() {
        let store = temp_store();
        store.set("DbPassword", "p@ss;word").expect("set");
        let r = SecretResolver::empty().with(EnvironmentSecrets).with(store.clone());
        assert_eq!(r.expand("Host=h;Password={secret:DbPassword};").expect("expand"), "Host=h;Password=p@ss;word;");
        let missing = r.resolve("Nope").expect_err("missing");
        assert!(matches!(missing, DataError::Secret(ref m) if m.contains("`Nope`") && m.contains("user secrets")));
        assert!(r.expand("{secret:Unclosed").is_err());
        let _ = std::fs::remove_dir_all(store.path().parent().expect("dir"));
    }

    #[test]
    fn environment_variable_names() {
        assert_eq!(EnvironmentSecrets::variable("ConnectionStrings:Main"), "ConnectionStrings__Main");
    }

    #[test]
    fn manifest_id() {
        let manifest = "[package]\nname = \"app\"\n\n[package.metadata.kubuno]\nuser-secrets-id = \"3f1c-app_1\" # dev\n";
        assert_eq!(user_secrets_id_from_manifest(manifest), Some("3f1c-app_1".to_string()));
        assert_eq!(user_secrets_id_from_manifest("[package.metadata.kubuno]\nuser-secrets-id = \"../x\"\n"), None);
        assert_eq!(user_secrets_id_from_manifest("[package]\nuser-secrets-id = \"a\"\n"), None);
        assert!(!is_valid_user_secrets_id(".."));
    }

    /// Writes, reads and deletes a clearly named test credential in the Windows Credential Manager.
    /// Ignored by default (it touches the user's credential store): `cargo test -- --ignored`.
    #[cfg(all(windows, feature = "credential-manager"))]
    #[test]
    #[ignore]
    fn credential_manager_round_trip() {
        let cm = CredentialManager::for_id(&format!("kubuno-data-test-{}", std::process::id())).expect("id");
        cm.set("ConnectionStrings:Test", "Host=localhost;Password=x").expect("set");
        assert_eq!(cm.get("ConnectionStrings:Test").expect("get"), Some("Host=localhost;Password=x".to_string()));
        assert!(cm.remove("ConnectionStrings:Test").expect("remove"));
        assert_eq!(cm.get("ConnectionStrings:Test").expect("get"), None);
        assert!(!cm.remove("ConnectionStrings:Test").expect("remove again"));
    }
}
