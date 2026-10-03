//! [`FileStore`]: the app's own files — isolated data, a cache with eviction, temporary files
//! (`docs/STORAGE-COMPONENTS.md` §3.4, lot ST-2) — and the location of its local databases ([`local_database_path`]).
//!
//! | Kind | Folder | Lifetime |
//! |---|---|---|
//! | `Data` | `<user_data_dir>/<app>/files/<name>/` (account-scoped: `accounts/<key>/<app>/files/<name>/`) | kept |
//! | `Cache` | `<user_cache_dir>/<app>/<name>/` | evicted, least recently used first, above `max_size` |
//! | `Temp` | `<temp>/Kubuno-<app>/<name>/` (`<sandbox>/temp/…` in a sandbox) | files older than a day removed when opened |
//!
//! A store takes **names**, never paths: 1–200 characters, none of `\ / : * ? " < > |` or a control character, not
//! `.`/`..`, no trailing dot or space, not a Windows device name (`con`, `nul`, `com1`…), so a name is valid on
//! Windows, macOS and Linux alike. Writes go through a temporary file renamed over the target; a read refreshes a
//! cache entry's modification time (its recency for the eviction).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::account::AccountKey;
use crate::app::AppId;
use crate::backend::file::replace;
use crate::{paths, StorageError};

/// The default size cap of a cache store.
pub const DEFAULT_CACHE_SIZE: u64 = 256 * 1024 * 1024;
/// How old a temporary file must be to be removed when a temp store is opened.
pub const TEMP_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// What a store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FileKind {
    #[default]
    Data,
    Cache,
    Temp,
}

/// One file of a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7",
    "lpt8", "lpt9",
];

/// Whether `name` is a valid file name on every OS (see the module doc).
pub fn valid_file_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim().to_ascii_lowercase();
    !name.is_empty()
        && name.chars().count() <= 200
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !name.ends_with(' ')
        && !name.chars().any(|c| c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        && !RESERVED.contains(&stem.as_str())
}

fn check(name: &str) -> Result<(), StorageError> {
    if valid_file_name(name) {
        Ok(())
    } else {
        Err(StorageError::InvalidName(format!("file name '{name}'")))
    }
}

fn dir_error(what: &str, e: std::io::Error) -> StorageError {
    StorageError::Backend { backend: "file", message: format!("the {what} directory: {e}") }
}

/// The app's files of one kind (see the module doc). Cheap to clone.
#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
    kind: FileKind,
    max_size: u64,
}

impl FileStore {
    /// The store `name` (a sub-folder, `files` by default in the components) of `app`.
    pub fn open(app: &AppId, name: &str, kind: FileKind, account: Option<&AccountKey>) -> Result<Self, StorageError> {
        check(name)?;
        let root = match kind {
            FileKind::Data => match account {
                Some(a) => crate::account::account_app_dir(a, app)?.join("files").join(name),
                None => paths::user_data_dir().map_err(|e| dir_error("data", e))?.join(app.as_str()).join("files").join(name),
            },
            FileKind::Cache => paths::user_cache_dir().map_err(|e| dir_error("cache", e))?.join(app.as_str()).join(name),
            FileKind::Temp => temp_dir()?.join(format!("Kubuno-{app}")).join(name),
        };
        let store = Self::at(root, kind);
        if kind == FileKind::Temp {
            store.remove_older_than(TEMP_MAX_AGE);
        }
        Ok(store)
    }

    /// A store in `root` (tests).
    pub fn at(root: PathBuf, kind: FileKind) -> Self {
        Self { root, kind, max_size: DEFAULT_CACHE_SIZE }
    }

    /// The size cap of a cache store (builder form).
    pub fn with_max_size(mut self, bytes: u64) -> Self {
        self.max_size = bytes;
        self
    }

    pub fn kind(&self) -> FileKind {
        self.kind
    }

    /// The store's folder (it may not exist yet).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path of `name` in the store, for an API that takes a path (a picture viewer, a database).
    pub fn path_of(&self, name: &str) -> Result<PathBuf, StorageError> {
        check(name)?;
        Ok(self.root.join(name))
    }

    /// Writes `name` (replacing it); a cache store then evicts what exceeds its cap.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let path = self.path_of(name)?;
        fs::create_dir_all(&self.root).map_err(|e| StorageError::io(&self.root, e))?;
        replace(&path, bytes)?;
        if self.kind == FileKind::Cache {
            self.evict()?;
        }
        Ok(())
    }

    /// The content of `name` (`None` when absent). A cache entry read becomes the most recently used.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.path_of(name)?;
        match fs::read(&path) {
            Ok(b) => {
                if self.kind == FileKind::Cache {
                    if let Ok(f) = fs::File::options().write(true).open(&path) {
                        let _ = f.set_modified(SystemTime::now());
                    }
                }
                Ok(Some(b))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::io(&path, e)),
        }
    }

    /// `name` as UTF-8 text.
    pub fn read_to_string(&self, name: &str) -> Result<Option<String>, StorageError> {
        self.read(name)?
            .map(|b| String::from_utf8(b).map_err(|_| StorageError::Corrupted(format!("{name} (not UTF-8 text)"))))
            .transpose()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path_of(name).is_ok_and(|p| p.is_file())
    }

    /// Removes `name`; `Ok(false)` when it was not there.
    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        let path = self.path_of(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::io(&path, e)),
        }
    }

    /// The files of the store, by name.
    pub fn list(&self) -> Result<Vec<FileInfo>, StorageError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StorageError::io(&self.root, e)),
        };
        let mut out: Vec<FileInfo> = entries
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok().filter(|m| m.is_file())?;
                let name = e.file_name().to_str()?.to_string();
                // A file being written (`<name>.<pid>.tmp`) is not part of the store yet.
                (!name.ends_with(".tmp")).then(|| FileInfo { name, size: m.len(), modified: m.modified().ok() })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The bytes the store holds.
    pub fn size(&self) -> Result<u64, StorageError> {
        Ok(self.list()?.iter().map(|f| f.size).sum())
    }

    /// Removes every file of the store.
    pub fn clear(&self) -> Result<usize, StorageError> {
        let mut n = 0;
        for f in self.list()? {
            if self.delete(&f.name)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// A cache above its cap: removes the least recently used files until it fits. Returns how many were removed.
    pub fn evict(&self) -> Result<usize, StorageError> {
        let mut files = self.list()?;
        let mut total: u64 = files.iter().map(|f| f.size).sum();
        if total <= self.max_size {
            return Ok(0);
        }
        files.sort_by_key(|f| f.modified.unwrap_or(SystemTime::UNIX_EPOCH));
        let mut removed = 0;
        for f in files {
            if total <= self.max_size {
                break;
            }
            if self.delete(&f.name)? {
                total = total.saturating_sub(f.size);
                removed += 1;
            }
        }
        tracing::debug!(target: "kubuno_app_storage", cache = %self.root.display(), removed, "cache evicted");
        Ok(removed)
    }

    fn remove_older_than(&self, age: Duration) {
        let Ok(files) = self.list() else { return };
        let now = SystemTime::now();
        for f in files {
            if f.modified.and_then(|m| now.duration_since(m).ok()).is_some_and(|d| d > age) {
                let _ = self.delete(&f.name);
            }
        }
    }
}

/// The temporary directory: the system's (`GetTempPath2W` through std), or `<sandbox>/temp` in a sandbox.
fn temp_dir() -> Result<PathBuf, StorageError> {
    Ok(match paths::sandbox_dir() {
        Some(s) => s.join("temp"),
        None => std::env::temp_dir(),
    })
}

/// The file of a local database (`docs/STORAGE-COMPONENTS.md` §3.5): a data source spec `app:<name>`,
/// `app:<app>/<name>`, `account:<name>` or `account:<app>/<name>` → `<user_data_dir>/<app>/databases/<name>.db`
/// (account: `<user_data_dir>/accounts/<key>/<app>/databases/<name>.db`, with the current account). The app defaults
/// to the process's ([`crate::default_app_id`]). `None` when `spec` is not one of these forms; the folder is created.
pub fn local_database_path(spec: &str) -> Option<Result<PathBuf, StorageError>> {
    let (scope, rest) = spec.trim().split_once(':')?;
    if scope != "app" && scope != "account" {
        return None;
    }
    Some((|| {
        let (app, name) = match rest.split_once('/') {
            Some((a, n)) => (AppId::new(a)?, n),
            None => (crate::default_app_id(), rest),
        };
        check(name)?;
        let dir = if scope == "account" {
            let account = crate::account::current_account().ok_or_else(|| StorageError::Setting { name: spec.to_string(), message: "no account is signed in".into() })?;
            crate::account::account_app_dir(&account, &app)?
        } else {
            paths::user_data_dir().map_err(|e| dir_error("data", e))?.join(app.as_str())
        }
        .join("databases");
        fs::create_dir_all(&dir).map_err(|e| StorageError::io(&dir, e))?;
        Ok(dir.join(format!("{name}.db")))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_portable() {
        for ok in ["a.txt", "Résumé 2026.pdf", "x", ".hidden", "console.log"] {
            assert!(valid_file_name(ok), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "a:b", "a?", "trail.", "trail ", "nul", "COM1.txt", "con.d", "a\u{7}b", &"x".repeat(201)] {
            assert!(!valid_file_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn data_files_and_cache_eviction() {
        let dir = tempfile::tempdir().expect("tmp");
        let data = FileStore::at(dir.path().join("data"), FileKind::Data);
        data.write("note.txt", b"hello").expect("write");
        assert_eq!(data.read_to_string("note.txt").expect("read").as_deref(), Some("hello"));
        assert!(data.exists("note.txt") && !data.exists("other"));
        assert!(data.write("../escape", b"x").is_err());
        assert_eq!(data.list().expect("list").len(), 1);
        assert!(data.delete("note.txt").expect("delete"));
        assert_eq!(data.read("note.txt").expect("read"), None);

        let cache = FileStore::at(dir.path().join("cache"), FileKind::Cache).with_max_size(25);
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            cache.write(name, &[0u8; 10]).expect("write");
            // Distinct modification times (some file systems keep 1 s or 2 s resolution).
            let f = fs::File::options().write(true).open(cache.path_of(name).expect("path")).expect("open");
            f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i as u64 * 10)).expect("time");
        }
        // 30 bytes > 25 after "c": the oldest ("a") went. Reading "b" makes it the most recent.
        let names: Vec<String> = cache.list().expect("list").into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["b", "c"]);
        cache.read("b").expect("read");
        cache.write("d", &[0u8; 10]).expect("write");
        let names: Vec<String> = cache.list().expect("list").into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["b", "d"], "the least recently used ('c') went");
        assert_eq!(cache.size().expect("size"), 20);
        assert_eq!(cache.clear().expect("clear"), 2);
    }

    #[test]
    fn database_specs() {
        assert!(local_database_path("Data Source=x").is_none());
        assert!(local_database_path("file:x").is_none());
        assert!(matches!(local_database_path("app:../x"), Some(Err(_))));
        assert!(matches!(local_database_path("app:Bad App/x"), Some(Err(_))));
    }
}
