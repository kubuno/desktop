//! [`FileBackend`]: settings as JSON files in the platform directories of [`crate::paths`].
//!
//! ```text
//! <user_config_dir>/<app>/<set>.settings.json         roaming user settings   (%APPDATA%\Kubuno\notes\settings.settings.json)
//! <user_data_dir>/<app>/<set>.local.settings.json     local user settings     (%LOCALAPPDATA%\Kubuno\notes\settings.local.settings.json)
//! <machine_config_dir>/<app>/<set>.settings.json      machine settings        (%ProgramData%\Kubuno\Desktop\notes\settings.settings.json)
//! ```
//!
//! The two user files have different names because on macOS the configuration and data directories are the same.
//! A file is a JSON object, keys sorted (stable diffs), `"$version"` holding the schema version:
//!
//! ```json
//! { "$version": 2, "RecentFiles": ["a.kbdoc"], "Theme": "Dark" }
//! ```
//!
//! Writes take an exclusive lock on `<file>.lock` (`File::lock`: `LockFileEx` / `flock`), read the file again,
//! apply the changes, write a temporary file and rename it over the old one: two instances saving at once both keep
//! their changes, and a crash never leaves a half-written file. Values the app does not understand (an object
//! written by a newer version) are kept as they are.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Change, SettingsBackend, StoredLayer};
use crate::app::AppId;
use crate::settings::{Layer, SettingValue, MAX_SET_BYTES};
use crate::{paths, StorageError};

/// The key of the schema version in a file (not a valid setting name, so it never collides with one).
pub const VERSION_KEY: &str = "$version";

/// The three directories a [`FileBackend`] writes below (`<root>/<app>/…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRoots {
    pub roaming: PathBuf,
    pub local: PathBuf,
    pub machine: PathBuf,
}

impl FileRoots {
    /// The platform directories of [`crate::paths`] (sandbox-aware).
    pub fn platform() -> Result<Self, StorageError> {
        let io = |what: &str, e: std::io::Error| StorageError::Backend { backend: "file", message: format!("the {what} directory: {e}") };
        Ok(Self {
            roaming: paths::user_config_dir().map_err(|e| io("configuration", e))?,
            local: paths::user_data_dir().map_err(|e| io("data", e))?,
            machine: paths::machine_config_dir().map_err(|e| io("machine configuration", e))?,
        })
    }

    /// The account-scoped roots (decision Q6): both user layers below `<user_data_dir>/accounts/<key>` (the files are
    /// `<key>/<app>/<set>.settings.json` and `.local.settings.json`), the machine layer unchanged.
    pub fn for_account(account: &crate::account::AccountKey) -> Result<Self, StorageError> {
        let platform = Self::platform()?;
        let dir = paths::user_data_dir().map_err(|e| StorageError::Backend { backend: "file", message: format!("the data directory: {e}") })?.join("accounts").join(account.as_str());
        Ok(Self { roaming: dir.clone(), local: dir, machine: platform.machine })
    }

    /// Everything below one directory (tests): `<dir>/roaming`, `<dir>/local`, `<dir>/machine`.
    pub fn under(dir: &Path) -> Self {
        Self { roaming: dir.join("roaming"), local: dir.join("local"), machine: dir.join("machine") }
    }
}

/// See the module doc.
#[derive(Debug, Clone)]
pub struct FileBackend {
    app: AppId,
    set: String,
    roots: FileRoots,
}

impl FileBackend {
    pub fn new(app: &AppId, set: &str, roots: FileRoots) -> Self {
        Self { app: app.clone(), set: set.to_string(), roots }
    }

    /// The file of `layer`.
    pub fn file(&self, layer: Layer) -> PathBuf {
        let (root, suffix) = match layer {
            Layer::UserRoaming => (&self.roots.roaming, "settings.json"),
            Layer::UserLocal => (&self.roots.local, "local.settings.json"),
            Layer::Machine => (&self.roots.machine, "settings.json"),
        };
        root.join(self.app.as_str()).join(format!("{}.{suffix}", self.set))
    }

    fn read_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, StorageError> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(serde_json::Map::new()),
            Err(e) => return Err(StorageError::io(path, e)),
        };
        if bytes.len() > MAX_SET_BYTES {
            return Err(StorageError::TooLarge { name: path.display().to_string(), len: bytes.len(), max: MAX_SET_BYTES });
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(serde_json::Map::new());
        }
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(map)) => Ok(map),
            // The parser's message may quote the content: only the position is reported.
            Err(e) => Err(StorageError::Corrupted(format!("{} (line {}, column {})", path.display(), e.line(), e.column()))),
            Ok(_) => Err(StorageError::Corrupted(format!("{} (not a JSON object)", path.display()))),
        }
    }
}

/// Holds `<file>.lock` exclusively while alive (released when the handle closes).
pub(crate) struct FileLock(#[allow(dead_code)] Option<fs::File>);

pub(crate) fn lock(path: &Path) -> Result<FileLock, StorageError> {
    let lock_path = path.with_extension("lock");
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path).map_err(|e| StorageError::io(&lock_path, e))?;
    match file.lock() {
        Ok(()) => Ok(FileLock(Some(file))),
        // Some network file systems do not lock: the atomic rename still protects the file itself.
        Err(e) => {
            tracing::debug!(target: "kubuno_app_storage", file = %lock_path.display(), "settings lock unavailable: {e}");
            Ok(FileLock(None))
        }
    }
}

/// Writes `bytes` to `path` through a temporary file renamed over it.
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(StorageError::io(path, e));
    }
    Ok(())
}

impl SettingsBackend for FileBackend {
    fn name(&self) -> &'static str {
        "file"
    }

    fn location(&self, layer: Layer) -> String {
        self.file(layer).display().to_string()
    }

    fn read(&self, layer: Layer) -> Result<StoredLayer, StorageError> {
        let path = self.file(layer);
        let map = Self::read_object(&path)?;
        let mut out = StoredLayer::default();
        for (k, v) in &map {
            if k == VERSION_KEY {
                out.version = v.as_u64().and_then(|v| u32::try_from(v).ok());
                continue;
            }
            match SettingValue::from_json(v) {
                Some(v) => {
                    out.values.insert(k.clone(), v);
                }
                None => tracing::debug!(target: "kubuno_app_storage", file = %path.display(), key = %k, "a stored value of another shape is kept untouched"),
            }
        }
        if out.version.is_none() && !map.is_empty() {
            out.version = Some(0);
        }
        Ok(out)
    }

    fn write(&self, layer: Layer, changes: &[Change], version: u32) -> Result<(), StorageError> {
        let path = self.file(layer);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| StorageError::io(dir, e))?;
        }
        let _guard = lock(&path)?;
        let mut map = Self::read_object(&path)?;
        for (k, v) in changes {
            match v {
                Some(v) => {
                    map.insert(k.clone(), v.to_json());
                }
                None => {
                    map.remove(k);
                }
            }
        }
        let stored = map.get(VERSION_KEY).and_then(serde_json::Value::as_u64).unwrap_or(0);
        map.insert(VERSION_KEY.to_string(), serde_json::Value::from(stored.max(u64::from(version))));
        let mut text = serde_json::to_vec_pretty(&serde_json::Value::Object(map)).map_err(|e| StorageError::Backend { backend: "file", message: e.to_string() })?;
        text.push(b'\n');
        if text.len() > MAX_SET_BYTES {
            return Err(StorageError::TooLarge { name: path.display().to_string(), len: text.len(), max: MAX_SET_BYTES });
        }
        replace(&path, &text)
    }

    fn stamp(&self, layer: Layer) -> Option<u64> {
        let m = fs::metadata(self.file(layer)).ok()?;
        let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
        Some(modified ^ m.len().rotate_left(48))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(dir: &Path) -> FileBackend {
        FileBackend::new(&AppId::new("test-app").expect("id"), "settings", FileRoots::under(dir))
    }

    #[test]
    fn files_are_merged_sorted_and_versioned() {
        let dir = tempfile::tempdir().expect("tmp");
        let b = backend(dir.path());
        assert_eq!(b.read(Layer::UserRoaming).expect("empty"), StoredLayer::default());
        assert!(b.stamp(Layer::UserRoaming).is_none());
        b.write(Layer::UserRoaming, &[("Zoom".into(), Some(SettingValue::Float(1.5))), ("Theme".into(), Some("Dark".into()))], 2).expect("write");
        // Another writer (another instance) adds a name; ours keep theirs.
        b.write(Layer::UserRoaming, &[("Other".into(), Some(SettingValue::Bool(true)))], 1).expect("write");
        let text = fs::read_to_string(b.file(Layer::UserRoaming)).expect("file");
        assert!(text.find("\"$version\": 2").is_some(), "{text}");
        assert!(text.find("\"Other\"") < text.find("\"Theme\"") && text.find("\"Theme\"") < text.find("\"Zoom\""), "sorted: {text}");
        let l = b.read(Layer::UserRoaming).expect("read");
        assert_eq!(l.version, Some(2), "a lower version never lowers the stored one");
        assert_eq!(l.values.len(), 3);
        b.write(Layer::UserRoaming, &[("Theme".into(), None)], 2).expect("remove");
        assert!(!b.read(Layer::UserRoaming).expect("read").values.contains_key("Theme"));
        assert!(b.stamp(Layer::UserRoaming).is_some());
        let leftovers: Vec<_> = fs::read_dir(b.file(Layer::UserRoaming).parent().expect("dir"))
            .expect("list")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temporary file is left behind");
    }

    #[test]
    fn foreign_values_are_kept_and_bad_files_reported_without_content() {
        let dir = tempfile::tempdir().expect("tmp");
        let b = backend(dir.path());
        let f = b.file(Layer::UserLocal);
        fs::create_dir_all(f.parent().expect("dir")).expect("mkdir");
        fs::write(&f, r#"{"Window": {"x": 1}, "Theme": "Light"}"#).expect("seed");
        assert_eq!(b.read(Layer::UserLocal).expect("read").version, Some(0), "values without a version are version 0");
        b.write(Layer::UserLocal, &[("Theme".into(), Some("Dark".into()))], 1).expect("write");
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&f).expect("file")).expect("json");
        assert_eq!(v["Window"]["x"], 1, "an object written by a newer version survives");
        fs::write(&f, "{\"Password\": \"hunter2\"").expect("broken");
        let err = b.read(Layer::UserLocal).expect_err("corrupted").to_string();
        assert!(err.contains("corrupted") && !err.contains("hunter2"), "{err}");
    }

    #[test]
    fn user_files_differ_where_directories_coincide() {
        let dir = tempfile::tempdir().expect("tmp");
        let same = FileRoots { roaming: dir.path().into(), local: dir.path().into(), machine: dir.path().join("m") };
        let b = FileBackend::new(&AppId::new("a").expect("id"), "settings", same);
        assert_ne!(b.file(Layer::UserRoaming), b.file(Layer::UserLocal));
    }
}
