//! Storable hierarchy — port of the `OwlCore.Storage` model plus the
//! `Files.Core.Storage.Storables` additions (`IDirectCopy`, `IDirectMove`).
//!
//! C# interface → Rust trait mapping:
//! `IStorable`→[`Storable`], `IStorableChild`→[`StorableChild`],
//! `IFile`/`IFileExtended`→[`File`], `IFolder`→[`Folder`],
//! `IModifiableFolder`→[`ModifiableFolder`], `IDirectCopy`→[`DirectCopy`],
//! `IDirectMove`→[`DirectMove`]. `IChildFile`/`IChildFolder` are the
//! `File + StorableChild` / `Folder + StorableChild` combinations.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::error::StorageResult;

bitflags::bitflags! {
    /// Port of `StorableKind` (`[Flags] enum : byte`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StorableKind: u8 {
        const FILES = 1;
        const FOLDERS = 2;
        const ALL = Self::FILES.bits() | Self::FOLDERS.bits();
    }
}

/// Port of `System.IO.FileAccess`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAccess {
    Read,
    Write,
    ReadWrite,
}

/// Port of `System.IO.FileShare` (subset actually used).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileShare {
    #[default]
    None,
    Read,
    Write,
    ReadWrite,
    Delete,
}

/// Byte stream returned by [`File::open_stream`]; unifies the C# `Stream`.
pub trait StorageStream: AsyncRead + AsyncWrite + AsyncSeek + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + AsyncSeek + Send + Unpin> StorageStream for T {}

pub type BoxedStream = Pin<Box<dyn StorageStream>>;

/// An item in a folder: either a file or a folder (replaces C# downcasting
/// between `IChildFile`/`IChildFolder`).
#[derive(Clone)]
pub enum StorableItem {
    File(Arc<dyn ChildFile>),
    Folder(Arc<dyn ChildFolder>),
}

impl StorableItem {
    pub fn as_storable(&self) -> &dyn Storable {
        match self {
            StorableItem::File(f) => f.as_storable(),
            StorableItem::Folder(f) => f.as_storable(),
        }
    }

    pub fn id(&self) -> &str {
        self.as_storable().id()
    }

    pub fn name(&self) -> &str {
        self.as_storable().name()
    }

    pub fn kind(&self) -> StorableKind {
        match self {
            StorableItem::File(_) => StorableKind::FILES,
            StorableItem::Folder(_) => StorableKind::FOLDERS,
        }
    }
}

/// Root of the storable hierarchy (port of `IStorable`).
pub trait Storable: Send + Sync {
    /// Stable identifier — for the Windows implementation this is the
    /// file-system path.
    fn id(&self) -> &str;

    /// Display name.
    fn name(&self) -> &str;

    /// Upcast helper (Rust has no implicit trait upcasting across traits).
    fn as_storable(&self) -> &dyn Storable;
}

/// A storable that has a parent (port of `IStorableChild`).
#[async_trait]
pub trait StorableChild: Storable {
    /// Port of `GetParentAsync`.
    async fn get_parent(&self) -> StorageResult<Option<Arc<dyn Folder>>>;
}

/// A file (port of `IFile` + `IFileExtended`).
#[async_trait]
pub trait File: Storable {
    /// Port of `OpenStreamAsync(FileAccess, FileShare)`.
    async fn open_stream(&self, access: FileAccess, share: FileShare) -> StorageResult<BoxedStream>;
}

/// A folder (port of `IFolder`).
#[async_trait]
pub trait Folder: Storable {
    /// Port of `GetItemsAsync` — streams items incrementally, which matters
    /// for large directories in the UI.
    fn get_items(&self, kind: StorableKind) -> BoxStream<'_, StorageResult<StorableItem>>;

    /// Port of the `GetFirstByNameAsync` extension.
    async fn get_first_by_name(&self, name: &str) -> StorageResult<StorableItem> {
        use futures::StreamExt;
        let mut items = self.get_items(StorableKind::ALL);
        while let Some(item) = items.next().await {
            let item = item?;
            if item.name().eq_ignore_ascii_case(name) {
                return Ok(item);
            }
        }
        Err(crate::StorageError::NotFound(name.to_string()))
    }
}

/// `IChildFile` / `IChildFolder` equivalents.
pub trait ChildFile: File + StorableChild {}
impl<T: File + StorableChild> ChildFile for T {}

pub trait ChildFolder: Folder + StorableChild {}
impl<T: Folder + StorableChild> ChildFolder for T {}

/// A folder that supports modification (port of `IModifiableFolder`).
#[async_trait]
pub trait ModifiableFolder: Folder {
    async fn create_file(&self, name: &str, overwrite: bool) -> StorageResult<Arc<dyn ChildFile>>;
    async fn create_folder(&self, name: &str, overwrite: bool) -> StorageResult<Arc<dyn ChildFolder>>;
    async fn delete(&self, item: &StorableItem) -> StorageResult<()>;
}

/// Direct server-side copy support (port of `IDirectCopy`).
#[async_trait]
pub trait DirectCopy: ModifiableFolder {
    /// Port of `CreateCopyOfAsync`.
    async fn create_copy_of(&self, item_to_copy: &StorableItem, overwrite: bool) -> StorageResult<StorableItem>;
}

/// Direct server-side move support (port of `IDirectMove`).
#[async_trait]
pub trait DirectMove: ModifiableFolder {
    /// Port of `MoveFromAsync`.
    async fn move_from(
        &self,
        item_to_move: &StorableItem,
        source: &dyn ModifiableFolder,
        overwrite: bool,
    ) -> StorageResult<StorableItem>;
}

/// Ports of the `StorageExtensions.Folder` helpers.
pub async fn try_get_file_by_name(folder: &dyn Folder, file_name: &str) -> Option<Arc<dyn ChildFile>> {
    match folder.get_first_by_name(file_name).await {
        Ok(StorableItem::File(f)) => Some(f),
        _ => None,
    }
}

pub async fn try_get_folder_by_name(folder: &dyn Folder, folder_name: &str) -> Option<Arc<dyn ChildFolder>> {
    match folder.get_first_by_name(folder_name).await {
        Ok(StorableItem::Folder(f)) => Some(f),
        _ => None,
    }
}

/// Port of `StorageExtensions.File.CopyContentsToAsync`.
pub async fn copy_contents_to(source: &dyn File, destination: &dyn File) -> StorageResult<u64> {
    let mut src = source.open_stream(FileAccess::Read, FileShare::Read).await?;
    let mut dst = destination.open_stream(FileAccess::Write, FileShare::None).await?;
    let copied = tokio::io::copy(&mut src, &mut dst).await?;
    Ok(copied)
}
