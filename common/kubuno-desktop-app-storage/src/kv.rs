//! [`KeyValueStore`]: small untyped values of an app (`docs/STORAGE-COMPONENTS.md` §3.3, lot ST-2) — a last-opened
//! folder, a dismissed tip, a cached server answer with a time to live — without declaring a schema.
//!
//! One JSON file per store: `<user_data_dir>/<app>/kv/<store>.json` (account-scoped:
//! `<user_data_dir>/accounts/<key>/<app>/kv/<store>.json`), written like the settings files (an exclusive lock, read
//! again, changed, written to a temporary file renamed over the old one), so two instances keep each other's keys.
//! `Persistence::Session` (or the `Memory` back-end) keeps the values in memory only.
//!
//! ```json
//! { "$version": 1, "entries": { "lastFolder": { "v": "C:\\Docs" }, "motd": { "v": {"text": "…"}, "exp": 1767225600000 } } }
//! ```
//!
//! Keys are 1–256 printable characters; a value is any JSON ≤ 1 MiB; a store ≤ 8 MiB. Expired entries read as
//! absent and are dropped at the next write. Never a secret: a `SecretStore` is for that.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::account::AccountKey;
use crate::app::AppId;
use crate::backend::file::{lock, replace};
use crate::{paths, StorageError};

/// The longest key.
pub const MAX_KEY: usize = 256;
/// The largest value (its JSON text).
pub const MAX_KV_VALUE_BYTES: usize = 1024 * 1024;
/// The largest store (its file).
pub const MAX_KV_STORE_BYTES: usize = 8 * 1024 * 1024;

/// Where a store's values live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Persistence {
    /// In a file of the app's data folder.
    #[default]
    Persistent,
    /// In memory, for the life of the process (`sessionStorage` on the web).
    Session,
}

/// Whether `key` is a valid key: 1–256 characters, none of them a control character.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.chars().count() <= MAX_KEY && !key.chars().any(char::is_control)
}

/// Whether `name` is a valid store name (a file stem): `[A-Za-z0-9._-]`, 1–64 characters, not starting with a dot.
pub fn valid_store_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    value: serde_json::Value,
    /// Expiry, milliseconds since the Unix epoch.
    expires: Option<u64>,
}

impl Entry {
    fn live(&self, now: u64) -> bool {
        self.expires.is_none_or(|e| e > now)
    }
}

#[derive(Debug)]
struct Inner {
    name: String,
    file: Option<PathBuf>,
    /// The values (the file's content as last read or written; the whole store in session mode).
    entries: Mutex<BTreeMap<String, Entry>>,
}

/// A store of small untyped values (see the module doc). Cheap to clone.
#[derive(Debug, Clone)]
pub struct KeyValueStore {
    inner: Arc<Inner>,
}

impl KeyValueStore {
    /// The store `name` of `app` (account-scoped when `account` is given), in a file or in memory.
    pub fn open(app: &AppId, name: &str, persistence: Persistence, account: Option<&AccountKey>) -> Result<Self, StorageError> {
        if !valid_store_name(name) {
            return Err(StorageError::InvalidName(format!("key/value store '{name}' (A-Z, a-z, 0-9, '.', '-', '_')")));
        }
        let file = match persistence {
            Persistence::Session => None,
            Persistence::Persistent => {
                let dir = match account {
                    Some(a) => crate::account::account_app_dir(a, app)?,
                    None => paths::user_data_dir().map_err(|e| StorageError::Backend { backend: "file", message: format!("the data directory: {e}") })?.join(app.as_str()),
                };
                Some(dir.join("kv").join(format!("{name}.json")))
            }
        };
        Self::at(name, file)
    }

    /// A store in memory (tests, the designer).
    pub fn in_memory(name: &str) -> Self {
        Self { inner: Arc::new(Inner { name: name.to_string(), file: None, entries: Mutex::new(BTreeMap::new()) }) }
    }

    /// A store in the file `file` (`None`: memory).
    pub fn at(name: &str, file: Option<PathBuf>) -> Result<Self, StorageError> {
        let s = Self { inner: Arc::new(Inner { name: name.to_string(), file, entries: Mutex::new(BTreeMap::new()) }) };
        s.reload()?;
        Ok(s)
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// The file behind the store (`None` in memory).
    pub fn location(&self) -> Option<&std::path::Path> {
        self.inner.file.as_deref()
    }

    fn read_file(file: &std::path::Path) -> Result<BTreeMap<String, Entry>, StorageError> {
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(StorageError::io(file, e)),
        };
        if bytes.len() > MAX_KV_STORE_BYTES {
            return Err(StorageError::TooLarge { name: file.display().to_string(), len: bytes.len(), max: MAX_KV_STORE_BYTES });
        }
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| StorageError::Corrupted(format!("{} (line {}, column {})", file.display(), e.line(), e.column())))?;
        let mut out = BTreeMap::new();
        if let Some(entries) = json.get("entries").and_then(serde_json::Value::as_object) {
            for (k, e) in entries {
                if let Some(v) = e.get("v") {
                    out.insert(k.clone(), Entry { value: v.clone(), expires: e.get("exp").and_then(serde_json::Value::as_u64) });
                }
            }
        }
        Ok(out)
    }

    /// Reads the file again (another instance's changes).
    pub fn reload(&self) -> Result<(), StorageError> {
        let Some(file) = &self.inner.file else { return Ok(()) };
        let fresh = Self::read_file(file)?;
        *self.entries() = fresh;
        Ok(())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.inner.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Applies `change` to the store: in memory, or to the file as it is now (read-merge-write under its lock).
    fn write(&self, change: impl Fn(&mut BTreeMap<String, Entry>)) -> Result<(), StorageError> {
        let Some(file) = &self.inner.file else {
            change(&mut self.entries());
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StorageError::io(dir, e))?;
        }
        let _guard = lock(file)?;
        let mut current = Self::read_file(file)?;
        change(&mut current);
        let now = now_ms();
        current.retain(|_, e| e.live(now));
        let entries: serde_json::Map<String, serde_json::Value> = current
            .iter()
            .map(|(k, e)| {
                let mut o = serde_json::Map::new();
                o.insert("v".into(), e.value.clone());
                if let Some(x) = e.expires {
                    o.insert("exp".into(), serde_json::Value::from(x));
                }
                (k.clone(), serde_json::Value::Object(o))
            })
            .collect();
        let doc = serde_json::json!({ "$version": 1, "entries": entries });
        let mut text = serde_json::to_vec_pretty(&doc).map_err(|e| StorageError::Backend { backend: "file", message: e.to_string() })?;
        text.push(b'\n');
        if text.len() > MAX_KV_STORE_BYTES {
            return Err(StorageError::TooLarge { name: file.display().to_string(), len: text.len(), max: MAX_KV_STORE_BYTES });
        }
        replace(file, &text)?;
        *self.entries() = current;
        Ok(())
    }

    /// The value of `key` (`None`: absent or expired).
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let now = now_ms();
        self.entries().get(key).filter(|e| e.live(now)).map(|e| e.value.clone())
    }

    /// The value of `key` as text (a JSON string, or the JSON text of any other value).
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key).map(|v| match v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        })
    }

    /// Sets `key`, optionally expiring after `ttl`; written at once.
    pub fn set(&self, key: &str, value: impl Into<serde_json::Value>, ttl: Option<Duration>) -> Result<(), StorageError> {
        if !valid_key(key) {
            return Err(StorageError::InvalidName(format!("key '{key}' (1 to {MAX_KEY} characters, no control character)")));
        }
        let value = value.into();
        let len = value.to_string().len();
        if len > MAX_KV_VALUE_BYTES {
            return Err(StorageError::TooLarge { name: key.to_string(), len, max: MAX_KV_VALUE_BYTES });
        }
        let expires = ttl.map(|t| now_ms().saturating_add(t.as_millis() as u64));
        let entry = Entry { value, expires };
        self.write(|m| {
            m.insert(key.to_string(), entry.clone());
        })
    }

    /// Removes `key`; returns whether it was there.
    pub fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let had = self.entries().contains_key(key);
        self.write(|m| {
            m.remove(key);
        })?;
        Ok(had)
    }

    /// The live keys, sorted.
    pub fn keys(&self) -> Vec<String> {
        let now = now_ms();
        self.entries().iter().filter(|(_, e)| e.live(now)).map(|(k, _)| k.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.keys().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every key.
    pub fn clear(&self) -> Result<(), StorageError> {
        self.write(BTreeMap::clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_ttl_and_two_instances() {
        let dir = tempfile::tempdir().expect("tmp");
        let file = dir.path().join("kv").join("main.json");
        let a = KeyValueStore::at("main", Some(file.clone())).expect("a");
        let b = KeyValueStore::at("main", Some(file.clone())).expect("b");
        a.set("lastFolder", "C:\\Docs", None).expect("set");
        a.set("motd", serde_json::json!({"text": "hi"}), Some(Duration::from_millis(1))).expect("set");
        b.set("tip.dismissed", true, None).expect("set");
        // `b` wrote after `a`: the file holds both instances' keys.
        a.reload().expect("reload");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(a.keys(), vec!["lastFolder".to_string(), "tip.dismissed".to_string()], "the expired entry reads as absent");
        assert_eq!(a.get_string("lastFolder").as_deref(), Some("C:\\Docs"));
        assert_eq!(a.get("tip.dismissed"), Some(serde_json::Value::Bool(true)));
        assert!(a.remove("lastFolder").expect("remove"));
        assert!(!a.remove("lastFolder").expect("again"));
        assert!(a.set("", 1, None).is_err() && a.set("a\nb", 1, None).is_err());
        let big = "x".repeat(MAX_KV_VALUE_BYTES + 1);
        assert!(matches!(a.set("big", big, None), Err(StorageError::TooLarge { .. })));
        a.clear().expect("clear");
        assert!(a.is_empty());
        let session = KeyValueStore::in_memory("s");
        session.set("k", 1, None).expect("set");
        assert_eq!(session.len(), 1);
        assert!(session.location().is_none());
    }
}
