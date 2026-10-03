//! [`MemoryBackend`]: settings that live as long as the process (tests, the designer, sample runs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use super::{Change, SettingsBackend, StoredLayer};
use crate::settings::{Layer, SettingValue};
use crate::StorageError;

/// See the module doc.
#[derive(Debug, Default)]
pub struct MemoryBackend {
    layers: Mutex<HashMap<Layer, StoredLayer>>,
    stamp: AtomicU64,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `values` in `layer` as if another process (or an administrator, for [`Layer::Machine`]) had
    /// written them: what tests use to simulate an external change.
    pub fn put(&self, layer: Layer, values: &[(&str, SettingValue)], version: Option<u32>) {
        let mut layers = self.layers.lock().unwrap_or_else(PoisonError::into_inner);
        let l = layers.entry(layer).or_default();
        for (k, v) in values {
            l.values.insert(k.to_string(), v.clone());
        }
        if version.is_some() {
            l.version = version;
        }
        self.stamp.fetch_add(1, Ordering::Relaxed);
    }

    /// What `layer` holds (tests).
    pub fn snapshot(&self, layer: Layer) -> StoredLayer {
        self.layers.lock().unwrap_or_else(PoisonError::into_inner).get(&layer).cloned().unwrap_or_default()
    }
}

impl SettingsBackend for MemoryBackend {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn location(&self, layer: Layer) -> String {
        format!("memory ({})", layer.name())
    }

    fn read(&self, layer: Layer) -> Result<StoredLayer, StorageError> {
        Ok(self.snapshot(layer))
    }

    fn write(&self, layer: Layer, changes: &[Change], version: u32) -> Result<(), StorageError> {
        let mut layers = self.layers.lock().unwrap_or_else(PoisonError::into_inner);
        let l = layers.entry(layer).or_default();
        for (k, v) in changes {
            match v {
                Some(v) => {
                    l.values.insert(k.clone(), v.clone());
                }
                None => {
                    l.values.remove(k);
                }
            }
        }
        l.version = Some(l.version.map_or(version, |v| v.max(version)));
        self.stamp.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn stamp(&self, layer: Layer) -> Option<u64> {
        let has = self.layers.lock().unwrap_or_else(PoisonError::into_inner).contains_key(&layer);
        has.then(|| self.stamp.load(Ordering::Relaxed))
    }
}
