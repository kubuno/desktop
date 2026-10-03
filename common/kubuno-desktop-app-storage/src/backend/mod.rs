//! Where settings are stored: the [`SettingsBackend`] trait and its implementations.
//!
//! | Back-end | Windows | macOS | Linux | Notes |
//! |---|---|---|---|---|
//! | [`FileBackend`] (the default, `Backend="Auto"` / `"File"`) | `%APPDATA%` / `%LOCALAPPDATA%` / `%ProgramData%` | `~/Library/Application Support`, `/Library/Application Support` | XDG config / data, `/etc/xdg` | JSON, one file per set and layer, atomic replace under a lock |
//! | [`RegistryBackend`] (`Backend="Registry"`) | `HKCU\Software\Kubuno\Apps`, `HKCU\Software\Classes\Local Settings\…`, `HKLM\Software\Kubuno\Apps` | — | — | for apps that integrate with Windows tooling (Group Policy preferences, `reg.exe` scripts) |
//! | [`MemoryBackend`] (`Backend="Memory"`) | ✓ | ✓ | ✓ | tests, the designer, `--sample` runs: nothing persists |
//!
//! A back-end stores raw values per [`Layer`]; the typing, the defaults, the scopes and the upgrade are the
//! engine's ([`crate::Settings`]), identical whatever the back-end.

use std::collections::BTreeMap;
use std::fmt;

use crate::settings::{Layer, SettingValue};
use crate::StorageError;

pub(crate) mod file;
mod memory;
#[cfg(windows)]
mod registry;

pub use file::{FileBackend, FileRoots};
pub use memory::MemoryBackend;
#[cfg(windows)]
pub use registry::RegistryBackend;

/// The stored values of one layer, with the schema version they were written for (`None`: nothing stored yet).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoredLayer {
    pub version: Option<u32>,
    pub values: BTreeMap<String, SettingValue>,
}

/// One change written by [`SettingsBackend::write`]: a value to store, or `None` to remove the name.
pub type Change = (String, Option<SettingValue>);

/// A place settings are kept. Implementations are blocking and thread-safe.
pub trait SettingsBackend: Send + Sync + fmt::Debug {
    /// For diagnostics (`"file"`, `"registry"`, `"memory"`).
    fn name(&self) -> &'static str;
    /// Where `layer` is kept (a file path, a Registry path) — for messages and the designer.
    fn location(&self, layer: Layer) -> String;
    /// The values of `layer` (an empty layer when nothing is stored).
    fn read(&self, layer: Layer) -> Result<StoredLayer, StorageError>;
    /// Applies `changes` to what `layer` holds **now** (read-merge-write: names this process never touched,
    /// including those of another version of the app, are kept), and records the schema `version` (never
    /// lowering a higher one).
    fn write(&self, layer: Layer, changes: &[Change], version: u32) -> Result<(), StorageError>;
    /// A number that changes when `layer` may have been changed by someone else (another instance, an
    /// administrator): the engine reloads when it differs. `None` when nothing is stored.
    fn stamp(&self, layer: Layer) -> Option<u64>;
}

/// The back-end a component or [`crate::SettingsOptions`] asks for (`Backend=` of `<Settings>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BackendKind {
    /// The platform's default: files on every OS (see `docs/STORAGE-COMPONENTS.md` §5.2 for why not the Registry).
    #[default]
    Auto,
    File,
    /// Windows only; elsewhere [`StorageError::Unsupported`].
    Registry,
    Memory,
}

impl BackendKind {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => BackendKind::Auto,
            "file" | "files" => BackendKind::File,
            "registry" => BackendKind::Registry,
            "memory" => BackendKind::Memory,
            _ => return None,
        })
    }
}
