//! `<FileStore>`: the app's own files (`kubuno_app_storage::FileStore`) — isolated data, a cache with eviction,
//! temporary files.
//!
//! ```xml
//! <FileStore x:Name="thumbs" Folder="thumbs" Kind="Cache" MaxSizeMb="64"/>
//! <Label Text="{Binding Count, Source=thumbs, StringFormat='{0} cached'}"/>
//! ```
//!
//! Files are named, never pathed (a name valid on every OS). Code: `self.thumbs.write("a.png", &bytes)`, `read`,
//! `delete`, `list`, `path_of`. Bindings: `Count`, `Size` (bytes), `Files` (rows with `Name` and `Size`), read-only.
//! In the designer nothing is read.

use kubuno_app_storage::{default_app_id, AppId, FileInfo, FileKind, FileStore as Engine, StorageError};
use kubuno_views::binding::{BindingFormat, Row, Value};
use kubuno_views::format::ValueKind;
use kubuno_views::prelude::*;
use kubuno_views::scope::{BindingProvider, ComponentScope};

/// What the store holds (`Kind=`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum FileStoreKind {
    /// Files the app keeps (its data folder).
    #[default]
    Data,
    /// Re-creatable files, evicted (least recently used first) above MaxSizeMb.
    Cache,
    /// Temporary files (removed after a day).
    Temp,
}

#[derive(Debug, Clone, PartialEq)]
struct Opened {
    folder: String,
    app_id: String,
    kind: FileStoreKind,
    max_size_mb: u32,
    account_scoped: bool,
    account_generation: u64,
}

/// The app's own files: data, a cache with eviction or temporary files, by name. Bindings: Count, Size, Files.
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "folder-archive", category = "Storage")]
#[default_property("Folder")]
pub struct FileStore {
    base: ComponentCore,
    /// The store's folder name below the app's data, cache or temporary folder.
    #[property]
    #[category("Storage")]
    #[default_value("files")]
    pub folder: String,
    /// Data (kept), Cache (evicted above MaxSizeMb, least recently used first) or Temp (removed after a day).
    #[property]
    #[category("Storage")]
    #[default_value("Data")]
    pub kind: FileStoreKind,
    /// A cache's size cap, in MiB.
    #[property]
    #[category("Storage")]
    #[default_value(256)]
    pub max_size_mb: u32,
    /// The app the files belong to (its id); empty for the application's own.
    #[property]
    #[category("Storage")]
    pub app_id: String,
    /// Whether the files belong to the signed-in account (Data only).
    #[property]
    #[category("Storage")]
    #[default_value(false)]
    pub account_scoped: bool,
    engine: Option<(Opened, Engine)>,
    /// `(count, size, files)` as read at the last refresh (bindings read from here).
    listed: Option<(usize, u64, Vec<FileInfo>)>,
    dirty: bool,
    last_error: Option<String>,
}

impl Default for FileStore {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            folder: "files".into(),
            kind: FileStoreKind::Data,
            max_size_mb: 256,
            app_id: String::new(),
            account_scoped: false,
            engine: None,
            listed: None,
            dirty: true,
            last_error: None,
        }
    }
}

impl std::fmt::Debug for FileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileStore").field("folder", &self.folder).field("kind", &self.kind).finish()
    }
}

impl FileStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(&self) -> Opened {
        Opened {
            folder: self.folder.trim().to_string(),
            app_id: self.app_id.trim().to_string(),
            kind: self.kind,
            max_size_mb: self.max_size_mb,
            account_scoped: self.account_scoped,
            account_generation: kubuno_app_storage::account::account_generation(),
        }
    }

    /// The store behind the component (opened on first use, reopened when a property or the account changed).
    pub fn engine(&mut self) -> Result<Engine, StorageError> {
        if crate::designing(self.design_mode()) {
            return Err(StorageError::Unsupported("a file store in the designer"));
        }
        let key = self.key();
        if let Some((k, e)) = &self.engine {
            if *k == key {
                return Ok(e.clone());
            }
        }
        let opened = (|| -> Result<Engine, StorageError> {
            let app = if key.app_id.is_empty() { default_app_id() } else { AppId::new(&key.app_id)? };
            let folder = if key.folder.is_empty() { "files" } else { key.folder.as_str() };
            let kind = match key.kind {
                FileStoreKind::Data => FileKind::Data,
                FileStoreKind::Cache => FileKind::Cache,
                FileStoreKind::Temp => FileKind::Temp,
            };
            let account = if key.account_scoped && kind == FileKind::Data {
                Some(kubuno_app_storage::current_account().ok_or_else(|| StorageError::Setting { name: folder.to_string(), message: "no account is signed in".into() })?)
            } else {
                None
            };
            Ok(Engine::open(&app, folder, kind, account.as_ref())?.with_max_size(u64::from(key.max_size_mb.max(1)) * 1024 * 1024))
        })();
        match opened {
            Ok(e) => {
                self.last_error = None;
                self.engine = Some((key, e.clone()));
                self.dirty = true;
                Ok(e)
            }
            Err(e) => {
                tracing::warn!(target: "kubuno_app_storage", component = %self.display_name(), "the file store cannot be opened: {e}");
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    pub fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let r = self.engine()?.write(name, bytes);
        self.dirty = true;
        r
    }

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.engine()?.read(name)
    }

    pub fn read_to_string(&mut self, name: &str) -> Result<Option<String>, StorageError> {
        self.engine()?.read_to_string(name)
    }

    pub fn delete(&mut self, name: &str) -> Result<bool, StorageError> {
        let r = self.engine()?.delete(name);
        self.dirty = true;
        r
    }

    pub fn list(&mut self) -> Result<Vec<FileInfo>, StorageError> {
        self.engine()?.list()
    }

    pub fn path_of(&mut self, name: &str) -> Result<std::path::PathBuf, StorageError> {
        self.engine()?.path_of(name)
    }

    pub fn clear(&mut self) -> Result<usize, StorageError> {
        let r = self.engine()?.clear();
        self.dirty = true;
        r
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Component for FileStore {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// `Count`, `Size`, `Files` (read-only).
impl BindingProvider for FileStore {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        let (count, size, files) = self.listed.as_ref()?;
        let v = match path {
            "Count" => Value::F32(*count as f32),
            "Size" => Value::F32(*size as f32),
            "Files" => Value::List(files.iter().map(|f| Row::new().with("Name", Value::Str(f.name.clone())).with("Size", Value::F32(f.size as f32))).collect::<Vec<_>>().into()),
            _ => return None,
        };
        kubuno_views::format::to_target(v, want, format)
    }

    fn binding_set(&mut self, _path: &str, _value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
        true
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        if crate::designing(self.design_mode()) {
            return false;
        }
        let reopened = self.engine.as_ref().map(|(k, _)| k.clone()) != Some(self.key());
        if !(self.dirty || reopened) {
            return false;
        }
        self.dirty = false;
        let listed = self.engine().and_then(|e| e.list()).map(|files| (files.len(), files.iter().map(|f| f.size).sum(), files)).ok();
        let changed = listed != self.listed;
        self.listed = listed;
        changed
    }
}
