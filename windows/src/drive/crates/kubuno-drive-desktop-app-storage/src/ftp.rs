//! Port of `Ftp/*` (`FtpHelpers`, `FtpManager`, `FtpStorable`,
//! `FtpStorageFile`, `FtpStorageFolder`, `FtpStorageService`).
//!
//! FluentFTP is replaced by the async (tokio) flavor of `suppaftp`.
//!
//! Deviation from C#: `FtpStorable.Id` upstream only keeps the FTP *path*
//! (e.g. `/pub/file.txt`), yet is later fed back into `GetFtpClient`, which
//! expects a full URL — host resolution is effectively broken upstream. The
//! Rust port keeps `id` as the FTP path for parity but additionally stores
//! the `authority` (`host[:port]`) captured at construction and uses it to
//! connect.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use futures::FutureExt;
use suppaftp::list::{File as FtpObject, ListParser};
use suppaftp::tokio::AsyncFtpStream;
use tokio::io::AsyncReadExt;

use kubuno_drive_desktop_core_storage::{
    BoxedStream, ChildFile, ChildFolder, DirectCopy, DirectMove, File, FileAccess, FileShare,
    Folder, ModifiableFolder, StorableItem, StorableKind, Storable, StorableChild, StorageError,
    StorageResult, StorageService,
};

// ---------------------------------------------------------------------------
// FtpManager
// ---------------------------------------------------------------------------

/// Port of `System.Net.NetworkCredential` (the subset used here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkCredential {
    pub username: String,
    pub password: String,
}

impl NetworkCredential {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self { username: username.into(), password: password.into() }
    }
}

/// Port of the static `FtpManager` — per-host credentials.
pub struct FtpManager;

impl FtpManager {
    /// Port of `FtpManager.Credentials`.
    pub fn credentials() -> &'static Mutex<HashMap<String, NetworkCredential>> {
        static CREDENTIALS: OnceLock<Mutex<HashMap<String, NetworkCredential>>> = OnceLock::new();
        CREDENTIALS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Port of `FtpManager.Anonymous`.
    pub fn anonymous() -> NetworkCredential {
        NetworkCredential::new("anonymous", "anonymous")
    }

    /// Looks up the credentials for `host`, defaulting to anonymous.
    pub fn credentials_for(host: &str) -> NetworkCredential {
        Self::credentials()
            .lock()
            .unwrap()
            .get(host)
            .cloned()
            .unwrap_or_else(Self::anonymous)
    }
}

// ---------------------------------------------------------------------------
// FtpHelpers
// ---------------------------------------------------------------------------

/// Port of `FtpHelpers.GetFtpPath`.
pub fn get_ftp_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let schema_index = match path.find("://") {
        Some(index) => index + 3,
        None => 2, // mirrors the C# `IndexOf(...) + 3` on failure (-1 + 3)
    };
    match path[schema_index.min(path.len())..].find('/') {
        Some(host_index) => path[schema_index + host_index..].to_string(),
        None => "/".to_string(),
    }
}

/// Port of `FtpHelpers.GetFtpAuthority`.
pub fn get_ftp_authority(path: &str) -> String {
    let path = path.replace('\\', "/");
    let schema_index = match path.find("://") {
        Some(index) => index + 3,
        None => 2,
    };
    let schema_index = schema_index.min(path.len());
    let host_index = path[schema_index..]
        .find('/')
        .map(|index| schema_index + index)
        .unwrap_or(path.len());
    path[schema_index..host_index].to_string()
}

/// Port of `FtpHelpers.GetFtpHost`.
pub fn get_ftp_host(path: &str) -> String {
    let authority = get_ftp_authority(path);
    match authority.find(':') {
        Some(index) => authority[..index].to_string(),
        None => authority,
    }
}

/// Port of `FtpHelpers.GetFtpPort`.
pub fn get_ftp_port(path: &str) -> u16 {
    let authority = get_ftp_authority(path);
    if let Some(index) = authority.find(':') {
        if let Ok(port) = authority[index + 1..].parse() {
            return port;
        }
    }
    if path.to_ascii_lowercase().starts_with("ftps://") {
        990
    } else {
        21
    }
}

fn ftp_error(error: suppaftp::FtpError) -> StorageError {
    StorageError::Other(format!("FTP error: {error}"))
}

/// Port of `FtpHelpers.GetFtpClient` + `EnsureConnectedAsync`: connects to the
/// authority (`host[:port]`) and logs in with the credentials registered in
/// [`FtpManager`].
pub async fn connect_client(authority: &str) -> StorageResult<AsyncFtpStream> {
    let host = match authority.find(':') {
        Some(index) => &authority[..index],
        None => authority,
    };
    let port: u16 = authority
        .find(':')
        .and_then(|index| authority[index + 1..].parse().ok())
        .unwrap_or(21);

    let mut client = AsyncFtpStream::connect((host, port)).await.map_err(ftp_error)?;

    let credential = FtpManager::credentials_for(host);
    client
        .login(&credential.username, &credential.password)
        .await
        .map_err(ftp_error)?;

    Ok(client)
}

/// Port of `FluentFTP.GetObjectInfo` — `MLST`, with a parent `LIST` fallback
/// for servers without MLST support.
async fn get_object_info(client: &mut AsyncFtpStream, path: &str) -> StorageResult<FtpObject> {
    if let Ok(line) = client.mlst(Some(path)).await {
        if let Ok(object) = ListParser::parse_mlst(&line) {
            return Ok(object);
        }
    }

    let (parent, name) = match path.rfind('/') {
        Some(0) => ("/", &path[1..]),
        Some(index) => (&path[..index], &path[index + 1..]),
        None => ("/", path),
    };

    let lines = client.list(Some(parent)).await.map_err(ftp_error)?;
    lines
        .iter()
        .filter_map(|line| FtpObject::try_from(line.as_str()).ok())
        .find(|object| object.name() == name)
        .ok_or_else(|| StorageError::NotFound(path.to_string()))
}

fn combine_path(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

// ---------------------------------------------------------------------------
// FtpStorable / FtpStorageFile / FtpStorageFolder
// ---------------------------------------------------------------------------

/// Port of the abstract `FtpStorable` base class.
#[derive(Debug, Clone)]
pub struct FtpStorable {
    /// The FTP path (`FtpHelpers.GetFtpPath`), like the C# `Id`.
    id: String,
    name: String,
    /// `host[:port]` used to connect (see the module-level deviation note).
    authority: String,
    /// The parent folder, if any.
    parent: Option<Arc<FtpStorageFolder>>,
}

impl FtpStorable {
    fn new(path: &str, name: impl Into<String>, authority: impl Into<String>, parent: Option<Arc<FtpStorageFolder>>) -> Self {
        Self {
            id: get_ftp_path(path),
            name: name.into(),
            authority: authority.into(),
            parent,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    async fn client(&self) -> StorageResult<AsyncFtpStream> {
        connect_client(&self.authority).await
    }
}

/// Port of `FtpStorageFile`.
#[derive(Debug, Clone)]
pub struct FtpStorageFile {
    inner: FtpStorable,
}

impl FtpStorageFile {
    pub fn new(path: &str, name: impl Into<String>, authority: impl Into<String>, parent: Option<Arc<FtpStorageFolder>>) -> Self {
        Self { inner: FtpStorable::new(path, name, authority, parent) }
    }
}

impl Storable for FtpStorageFile {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn as_storable(&self) -> &dyn Storable {
        self
    }
}

#[async_trait]
impl StorableChild for FtpStorageFile {
    async fn get_parent(&self) -> StorageResult<Option<Arc<dyn Folder>>> {
        Ok(self.inner.parent.clone().map(|parent| parent as Arc<dyn Folder>))
    }
}

#[async_trait]
impl File for FtpStorageFile {
    /// Port of `OpenStreamAsync`.
    ///
    /// Reading downloads the remote file into an in-memory cursor. Writing is
    /// not supported yet.
    /// TODO: support write access with an upload-on-shutdown stream wrapper.
    async fn open_stream(&self, access: FileAccess, _share: FileShare) -> StorageResult<BoxedStream> {
        match access {
            FileAccess::Read => {
                let mut client = self.inner.client().await?;
                let mut data = client.retr_as_stream(self.inner.id()).await.map_err(ftp_error)?;
                let mut buffer = Vec::new();
                data.read_to_end(&mut buffer).await?;
                client.finalize_retr_stream(data).await.map_err(ftp_error)?;
                let _ = client.quit().await;
                Ok(Box::pin(std::io::Cursor::new(buffer)) as BoxedStream)
            }
            FileAccess::Write | FileAccess::ReadWrite => Err(StorageError::NotSupported(
                "write access to FTP files is not supported yet".into(),
            )),
        }
    }
}

/// Port of `FtpStorageFolder`.
#[derive(Debug, Clone)]
pub struct FtpStorageFolder {
    inner: FtpStorable,
}

impl FtpStorageFolder {
    pub fn new(path: &str, name: impl Into<String>, authority: impl Into<String>, parent: Option<Arc<FtpStorageFolder>>) -> Self {
        Self { inner: FtpStorable::new(path, name, authority, parent) }
    }

    /// Lists the child items of this folder.
    async fn list_items(&self, kind: StorableKind) -> StorageResult<Vec<StorableItem>> {
        let mut client = self.inner.client().await?;
        let lines = client.list(Some(self.inner.id())).await.map_err(ftp_error)?;
        let _ = client.quit().await;

        let parent = Arc::new(self.clone());
        let mut items = Vec::new();
        for line in &lines {
            let Ok(object) = FtpObject::try_from(line.as_str()) else {
                continue;
            };
            let full_path = combine_path(self.inner.id(), object.name());
            if object.is_file() && kind.contains(StorableKind::FILES) {
                items.push(StorableItem::File(Arc::new(FtpStorageFile::new(
                    &full_path,
                    object.name(),
                    self.inner.authority(),
                    Some(Arc::clone(&parent)),
                ))));
            } else if object.is_directory() && kind.contains(StorableKind::FOLDERS) {
                items.push(StorableItem::Folder(Arc::new(FtpStorageFolder::new(
                    &full_path,
                    object.name(),
                    self.inner.authority(),
                    Some(Arc::clone(&parent)),
                ))));
            }
        }
        Ok(items)
    }
}

impl Storable for FtpStorageFolder {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn as_storable(&self) -> &dyn Storable {
        self
    }
}

#[async_trait]
impl StorableChild for FtpStorageFolder {
    async fn get_parent(&self) -> StorageResult<Option<Arc<dyn Folder>>> {
        Ok(self.inner.parent.clone().map(|parent| parent as Arc<dyn Folder>))
    }
}

impl Folder for FtpStorageFolder {
    /// Port of `GetItemsAsync`.
    fn get_items(&self, kind: StorableKind) -> BoxStream<'_, StorageResult<StorableItem>> {
        async move {
            match self.list_items(kind).await {
                Ok(items) => stream::iter(items.into_iter().map(Ok)).boxed(),
                Err(error) => stream::once(async move { Err(error) }).boxed(),
            }
        }
        .flatten_stream()
        .boxed()
    }
}

#[async_trait]
impl ModifiableFolder for FtpStorageFolder {
    /// Port of `CreateFileAsync` (including its upstream overwrite-check
    /// semantics).
    async fn create_file(&self, name: &str, overwrite: bool) -> StorageResult<Arc<dyn ChildFile>> {
        let mut client = self.inner.client().await?;
        let new_path = format!("{}/{name}", self.inner.id());

        let exists = get_object_info(&mut client, &new_path).await.is_ok();
        // NOTE: faithful to the C# port: `overwrite == true` throws when the
        // file already exists, and `overwrite == false` skips (throws) too.
        if exists && overwrite {
            let _ = client.quit().await;
            return Err(StorageError::AlreadyExists(name.to_string()));
        }
        if exists && !overwrite {
            let _ = client.quit().await;
            return Err(StorageError::Other(
                "Couldn't generate unique name. File skipped.".into(),
            ));
        }

        let mut empty: &[u8] = &[];
        client.put_file(&new_path, &mut empty).await.map_err(ftp_error)?;
        let _ = client.quit().await;

        Ok(Arc::new(FtpStorageFile::new(
            &new_path,
            name,
            self.inner.authority(),
            Some(Arc::new(self.clone())),
        )))
    }

    /// Port of `CreateFolderAsync`.
    async fn create_folder(&self, name: &str, overwrite: bool) -> StorageResult<Arc<dyn ChildFolder>> {
        let mut client = self.inner.client().await?;
        let new_path = format!("{}/{name}", self.inner.id());

        if overwrite && get_object_info(&mut client, &new_path).await.is_ok() {
            let _ = client.quit().await;
            return Err(StorageError::AlreadyExists(name.to_string()));
        }

        client.mkdir(&new_path).await.map_err(ftp_error)?;
        let _ = client.quit().await;

        Ok(Arc::new(FtpStorageFolder::new(
            &new_path,
            name,
            self.inner.authority(),
            Some(Arc::new(self.clone())),
        )))
    }

    /// Port of `DeleteAsync`.
    async fn delete(&self, item: &StorableItem) -> StorageResult<()> {
        let mut client = self.inner.client().await?;
        let result = match item {
            StorableItem::File(file) => client.rm(file.id()).await.map_err(ftp_error),
            StorableItem::Folder(folder) => client.rmdir(folder.id()).await.map_err(ftp_error),
        };
        let _ = client.quit().await;
        result
    }
}

#[async_trait]
impl DirectCopy for FtpStorageFolder {
    /// Port of `CreateCopyOfAsync` — download the source contents, upload to
    /// the new path (folders are not supported, same as C#).
    async fn create_copy_of(&self, item_to_copy: &StorableItem, _overwrite: bool) -> StorageResult<StorableItem> {
        let StorableItem::File(source_file) = item_to_copy else {
            return Err(StorageError::NotSupported("Copying folders is not supported.".into()));
        };

        // Read the source contents through the storable abstraction (the
        // source may live on a different storage).
        let mut source_stream = source_file.open_stream(FileAccess::Read, FileShare::Read).await?;
        let mut contents = Vec::new();
        source_stream.read_to_end(&mut contents).await?;

        let new_path = format!("{}/{}", self.inner.id(), source_file.name());
        let mut client = self.inner.client().await?;
        client.put_file(&new_path, &mut contents.as_slice()).await.map_err(ftp_error)?;
        let _ = client.quit().await;

        Ok(StorableItem::File(Arc::new(FtpStorageFile::new(
            &new_path,
            source_file.name(),
            self.inner.authority(),
            Some(Arc::new(self.clone())),
        ))))
    }
}

#[async_trait]
impl DirectMove for FtpStorageFolder {
    /// Port of `MoveFromAsync` — copy then delete from the source.
    async fn move_from(
        &self,
        item_to_move: &StorableItem,
        source: &dyn ModifiableFolder,
        overwrite: bool,
    ) -> StorageResult<StorableItem> {
        let new_item = self.create_copy_of(item_to_move, overwrite).await?;
        source.delete(item_to_move).await?;
        Ok(new_item)
    }
}

// ---------------------------------------------------------------------------
// FtpStorageService
// ---------------------------------------------------------------------------

/// Port of `FtpStorageService : IFtpStorageService`.
#[derive(Debug, Default)]
pub struct FtpStorageService;

impl FtpStorageService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StorageService for FtpStorageService {
    /// Port of `GetFileAsync`; `id` is a full FTP URL
    /// (`ftp://host[:port]/path`).
    async fn get_file(&self, id: &str) -> StorageResult<Arc<dyn ChildFile>> {
        let authority = get_ftp_authority(id);
        let ftp_path = get_ftp_path(id);

        let mut client = connect_client(&authority).await?;
        let object = get_object_info(&mut client, &ftp_path).await?;
        let _ = client.quit().await;

        if !object.is_file() {
            return Err(StorageError::NotFound("File was not found from path.".into()));
        }
        Ok(Arc::new(FtpStorageFile::new(&ftp_path, object.name(), authority, None)))
    }

    /// Port of `GetFolderAsync`.
    async fn get_folder(&self, id: &str) -> StorageResult<Arc<dyn ChildFolder>> {
        let authority = get_ftp_authority(id);
        let ftp_path = get_ftp_path(id);

        let mut client = connect_client(&authority).await?;
        let object = get_object_info(&mut client, &ftp_path).await?;
        let _ = client.quit().await;

        if !object.is_directory() {
            return Err(StorageError::NotFound("Directory was not found from path.".into()));
        }
        Ok(Arc::new(FtpStorageFolder::new(&ftp_path, object.name(), authority, None)))
    }
}

impl kubuno_drive_desktop_core_storage::service::FtpStorageService for FtpStorageService {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ftp_path_extraction() {
        assert_eq!(get_ftp_path("ftp://host/pub/file.txt"), "/pub/file.txt");
        assert_eq!(get_ftp_path("ftp://host:2121/pub"), "/pub");
        assert_eq!(get_ftp_path("ftp://host"), "/");
        assert_eq!(get_ftp_path("ftp:\\\\host\\pub"), "/pub");
    }

    #[test]
    fn ftp_authority_host_port() {
        assert_eq!(get_ftp_authority("ftp://host:2121/pub"), "host:2121");
        assert_eq!(get_ftp_host("ftp://host:2121/pub"), "host");
        assert_eq!(get_ftp_port("ftp://host:2121/pub"), 2121);
        assert_eq!(get_ftp_port("ftp://host/pub"), 21);
        assert_eq!(get_ftp_port("ftps://host/pub"), 990);
    }

    #[test]
    fn manager_defaults_to_anonymous() {
        let credential = FtpManager::credentials_for("nonexistent.example");
        assert_eq!(credential, FtpManager::anonymous());

        FtpManager::credentials()
            .lock()
            .unwrap()
            .insert("known.example".into(), NetworkCredential::new("user", "pass"));
        assert_eq!(
            FtpManager::credentials_for("known.example"),
            NetworkCredential::new("user", "pass")
        );
    }
}
