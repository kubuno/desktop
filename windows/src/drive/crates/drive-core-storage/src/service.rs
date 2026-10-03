//! Port of `IStorageService` / `IFtpStorageService` and the
//! `StorageExtensions.Service` try-helpers.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::StorageResult;
use crate::storables::{ChildFile, ChildFolder, StorableItem};

/// Abstract access to a file system (port of `IStorageService`).
/// `id` is implementation-defined — a path for the Windows service, an FTP
/// path for the FTP service.
#[async_trait]
pub trait StorageService: Send + Sync {
    async fn get_file(&self, id: &str) -> StorageResult<Arc<dyn ChildFile>>;
    async fn get_folder(&self, id: &str) -> StorageResult<Arc<dyn ChildFolder>>;

    /// Port of `TryGetStorableAsync` — folder first, then file.
    async fn try_get_storable(&self, id: &str) -> Option<StorableItem> {
        if let Ok(folder) = self.get_folder(id).await {
            return Some(StorableItem::Folder(folder));
        }
        self.get_file(id).await.ok().map(StorableItem::File)
    }
}

/// Marker for FTP storage services (port of `IFtpStorageService`).
pub trait FtpStorageService: StorageService {}
