//! Port of `Windows/WindowsStorable.cs`, `WindowsFile.cs`, `WindowsFolder.cs`
//! and the `IWindowsStorable`/`IWindowsFile`/`IWindowsFolder` interfaces.
//!
//! A [`WindowsStorable`] owns an `IShellItem`; windows-rs handles the COM
//! AddRef/Release pairing through `Clone`/`Drop`, replacing the manual
//! `Release()` calls in the C# `Dispose` implementations.
//!
//! COM threading: shell items are apartment-bound COM objects. The calling
//! thread must have COM initialized (`CoInitializeEx`/`OleInitialize`); use
//! [`crate::sta_thread`] for calls that require an STA (interactive shell
//! verbs, thumbnails, …). Marking these types `Send`/`Sync` mirrors the C#
//! code, which freely stores raw `IShellItem*` pointers on objects shared
//! between threads.

use std::fmt;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use windows::core::{GUID, HSTRING, PWSTR};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemServices::SFGAO_FOLDER;
use windows::Win32::UI::Shell::{
    IContextMenu, IEnumShellItems, IShellItem, SHCreateItemFromParsingName, SHGetKnownFolderItem,
    BHID_EnumItems, KF_FLAG_DEFAULT, SICHINT_DISPLAY, SIGDN, SIGDN_FILESYSPATH,
    SIGDN_PARENTRELATIVEFORUI,
};

use kubuno_drive_desktop_core_storage::{
    ChildFile, ChildFolder, File, FileAccess, FileShare, Folder, StorableItem, StorableKind,
    Storable, StorableChild, StorageError, StorageResult,
};

/// Converts a shell-allocated `PWSTR` into a `String` and frees it.
pub(crate) fn take_co_task_string(pwstr: PWSTR) -> String {
    if pwstr.is_null() {
        return String::new();
    }
    // SAFETY: `pwstr` is a valid NUL-terminated string allocated by the shell;
    // it is freed exactly once below.
    unsafe {
        let text = pwstr.to_string().unwrap_or_default();
        CoTaskMemFree(Some(pwstr.as_ptr() as _));
        text
    }
}

/// `GetDisplayName` helper used before a [`WindowsStorable`] exists.
/// Port of `WindowsStorableHelpers.GetDisplayName` (returns an empty string on failure).
pub(crate) fn display_name_of(item: &IShellItem, sigdn: SIGDN) -> String {
    // SAFETY: `item` is a live shell item; the returned string is released via
    // `take_co_task_string`.
    unsafe {
        match item.GetDisplayName(sigdn) {
            Ok(pwstr) => take_co_task_string(pwstr),
            Err(_) => String::new(),
        }
    }
}

/// Returns whether the shell item has the `SFGAO_FOLDER` attribute.
fn is_folder(item: &IShellItem) -> bool {
    // SAFETY: `item` is a live shell item.
    unsafe {
        item.GetAttributes(SFGAO_FOLDER)
            .map(|attrs| attrs == SFGAO_FOLDER)
            .unwrap_or(false)
    }
}

/// Either a [`WindowsFile`] or a [`WindowsFolder`]; the result of
/// `WindowsStorable.TryParse` in C# (which returns the abstract base type).
pub enum WindowsStorableItem {
    File(WindowsFile),
    Folder(WindowsFolder),
}

impl WindowsStorableItem {
    pub fn storable(&self) -> &WindowsStorable {
        match self {
            WindowsStorableItem::File(file) => &file.inner,
            WindowsStorableItem::Folder(folder) => &folder.inner,
        }
    }

    /// Converts into the crate-agnostic [`StorableItem`] used by the
    /// `kubuno-drive-desktop-core-storage` traits.
    pub fn into_storable_item(self) -> StorableItem {
        match self {
            WindowsStorableItem::File(file) => StorableItem::File(Arc::new(file)),
            WindowsStorableItem::Folder(folder) => StorableItem::Folder(Arc::new(folder)),
        }
    }
}

/// Port of the abstract `WindowsStorable` base class.
pub struct WindowsStorable {
    item: IShellItem,
    /// `Id` — `GetDisplayName(SIGDN_FILESYSPATH)`, empty for virtual items.
    id: String,
    /// `Name` — `GetDisplayName(SIGDN_PARENTRELATIVEFORUI)`.
    name: String,
    /// Cached `IContextMenu` (the C# `ContextMenu` property).
    context_menu: Mutex<Option<IContextMenu>>,
}

// SAFETY: mirrors the C# code, which stores raw `IShellItem*` pointers on
// objects used across threads. Callers are responsible for honoring COM
// apartment rules (see module docs).
unsafe impl Send for WindowsStorable {}
unsafe impl Sync for WindowsStorable {}

impl WindowsStorable {
    /// Wraps an existing shell item (the `WindowsStorable(IShellItem*)` path).
    pub fn from_shell_item(item: IShellItem) -> Self {
        let id = display_name_of(&item, SIGDN_FILESYSPATH);
        let name = display_name_of(&item, SIGDN_PARENTRELATIVEFORUI);
        Self { item, id, name, context_menu: Mutex::new(None) }
    }

    /// Port of `WindowsStorable.TryParse(string)`:
    /// `SHCreateItemFromParsingName` + folder/file dispatch.
    pub fn try_parse(path: &str) -> Option<WindowsStorableItem> {
        // SAFETY: standard shell parsing call; the returned interface is owned
        // by the wrapper.
        let item: IShellItem =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }.ok()?;
        Some(Self::from_item_dispatch(item))
    }

    /// Port of `WindowsStorable.TryParse(IShellItem*)`: dispatches on
    /// `SFGAO_FOLDER`.
    pub fn from_item_dispatch(item: IShellItem) -> WindowsStorableItem {
        if is_folder(&item) {
            WindowsStorableItem::Folder(WindowsFolder::new(item))
        } else {
            WindowsStorableItem::File(WindowsFile::new(item))
        }
    }

    /// Raw access to the owned `IShellItem` (the C# `ThisPtr` property).
    pub fn shell_item(&self) -> &IShellItem {
        &self.item
    }

    /// Stable identifier: the file-system path (empty for virtual items).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The cached `IContextMenu` (C# `ContextMenu` property).
    pub fn context_menu(&self) -> Option<IContextMenu> {
        self.context_menu.lock().unwrap().clone()
    }

    pub fn set_context_menu(&self, menu: Option<IContextMenu>) {
        *self.context_menu.lock().unwrap() = menu;
    }

    /// Port of `GetDisplayName(SIGDN)`; returns an empty string on failure.
    pub fn get_display_name(&self, sigdn: SIGDN) -> String {
        display_name_of(&self.item, sigdn)
    }

    /// Port of `GetParentAsync` (sync at this layer; the trait impls wrap it).
    pub fn parent(&self) -> Option<WindowsFolder> {
        // SAFETY: `self.item` is a live shell item.
        let parent = unsafe { self.item.GetParent() }.ok()?;
        Some(WindowsFolder::new(parent))
    }
}

impl PartialEq for WindowsStorable {
    /// Port of `Equals(IWindowsStorable)` — `IShellItem::Compare` with
    /// `SICHINT_DISPLAY`.
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: both items are live shell items.
        unsafe {
            self.item
                .Compare(&other.item, SICHINT_DISPLAY.0 as u32)
                .map(|order| order == 0)
                .unwrap_or(false)
        }
    }
}

impl Eq for WindowsStorable {}

impl fmt::Display for WindowsStorable {
    /// Port of `ToString()` → `GetDisplayName(SIGDN_FILESYSPATH)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

impl fmt::Debug for WindowsStorable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsStorable")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

/// Port of `WindowsFile` (`IWindowsFile`).
pub struct WindowsFile {
    inner: WindowsStorable,
}

impl WindowsFile {
    pub fn new(item: IShellItem) -> Self {
        Self { inner: WindowsStorable::from_shell_item(item) }
    }
}

impl std::ops::Deref for WindowsFile {
    type Target = WindowsStorable;

    fn deref(&self) -> &WindowsStorable {
        &self.inner
    }
}

/// Port of `WindowsFolder` (`IWindowsFolder`).
pub struct WindowsFolder {
    inner: WindowsStorable,
    /// Cached `IContextMenu` for the ShellNew context menu (C# `ShellNewMenu`).
    shell_new_menu: Mutex<Option<IContextMenu>>,
}

// SAFETY: see `WindowsStorable`.
unsafe impl Send for WindowsFolder {}
unsafe impl Sync for WindowsFolder {}

impl WindowsFolder {
    pub fn new(item: IShellItem) -> Self {
        Self {
            inner: WindowsStorable::from_shell_item(item),
            shell_new_menu: Mutex::new(None),
        }
    }

    /// Port of `WindowsFolder(Guid folderId)`: `SHGetKnownFolderItem` with a
    /// `Shell:::{guid}` parsing-name fallback.
    pub fn from_known_folder(folder_id: GUID) -> windows::core::Result<Self> {
        // SAFETY: standard known-folder lookup.
        let item: windows::core::Result<IShellItem> =
            unsafe { SHGetKnownFolderItem(&folder_id, KF_FLAG_DEFAULT, None) };
        let item = match item {
            Ok(item) => item,
            Err(_) => {
                let shell_path = format!("Shell:::{{{folder_id:?}}}");
                // SAFETY: standard shell parsing call.
                unsafe { SHCreateItemFromParsingName(&HSTRING::from(shell_path.as_str()), None)? }
            }
        };
        Ok(Self::new(item))
    }

    /// The cached ShellNew `IContextMenu` (C# `ShellNewMenu` property).
    pub fn shell_new_menu(&self) -> Option<IContextMenu> {
        self.shell_new_menu.lock().unwrap().clone()
    }

    pub fn set_shell_new_menu(&self, menu: Option<IContextMenu>) {
        *self.shell_new_menu.lock().unwrap() = menu;
    }

    /// Synchronous enumeration backing `GetItemsAsync`:
    /// `BindToHandler(BHID_EnumItems)` → `IEnumShellItems`.
    /// Like the C# version, failures produce an empty list.
    pub fn enumerate_items(&self, kind: StorableKind) -> Vec<WindowsStorableItem> {
        // SAFETY: `self` owns a live shell item; enumerated children are owned
        // by the produced wrappers.
        unsafe {
            let enumerator: IEnumShellItems =
                match self.inner.item.BindToHandler(None, &BHID_EnumItems) {
                    Ok(enumerator) => enumerator,
                    Err(error) => {
                        tracing::warn!(?error, folder = %self.inner, "failed to bind BHID_EnumItems");
                        return Vec::new();
                    }
                };

            let mut children = Vec::new();
            loop {
                let mut slot = [None];
                let mut fetched = 0u32;
                if enumerator.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(item) = slot[0].take() else { break };

                let child_is_folder = is_folder(&item);
                if kind.contains(StorableKind::FILES) && !child_is_folder {
                    children.push(WindowsStorableItem::File(WindowsFile::new(item)));
                } else if kind.contains(StorableKind::FOLDERS) && child_is_folder {
                    children.push(WindowsStorableItem::Folder(WindowsFolder::new(item)));
                }
            }

            children
        }
    }
}

impl std::ops::Deref for WindowsFolder {
    type Target = WindowsStorable;

    fn deref(&self) -> &WindowsStorable {
        &self.inner
    }
}

// ---------------------------------------------------------------------------
// kubuno-drive-desktop-core-storage trait implementations
// ---------------------------------------------------------------------------

impl Storable for WindowsFile {
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

impl Storable for WindowsFolder {
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
impl StorableChild for WindowsFile {
    async fn get_parent(&self) -> StorageResult<Option<Arc<dyn Folder>>> {
        Ok(self.inner.parent().map(|parent| Arc::new(parent) as Arc<dyn Folder>))
    }
}

#[async_trait]
impl StorableChild for WindowsFolder {
    async fn get_parent(&self) -> StorageResult<Option<Arc<dyn Folder>>> {
        Ok(self.inner.parent().map(|parent| Arc::new(parent) as Arc<dyn Folder>))
    }
}

#[async_trait]
impl File for WindowsFile {
    async fn open_stream(&self, _access: FileAccess, _share: FileShare) -> StorageResult<kubuno_drive_desktop_core_storage::BoxedStream> {
        // The C# `WindowsFile.OpenStreamAsync` throws `NotImplementedException`.
        Err(StorageError::NotSupported("WindowsFile::open_stream is not implemented".into()))
    }
}

impl Folder for WindowsFolder {
    fn get_items(&self, kind: StorableKind) -> BoxStream<'_, StorageResult<StorableItem>> {
        let items = self.enumerate_items(kind);
        stream::iter(items.into_iter().map(|item| Ok(item.into_storable_item()))).boxed()
    }
}

// `ChildFile`/`ChildFolder` come from the blanket impls in kubuno-drive-desktop-core-storage.
const _: () = {
    fn assert_impls<T: ChildFile>() {}
    fn assert_folder_impls<T: ChildFolder>() {}
    fn check() {
        assert_impls::<WindowsFile>();
        assert_folder_impls::<WindowsFolder>();
    }
    let _ = check;
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::init_com;

    #[test]
    fn parse_known_path_is_folder() {
        init_com();
        let parsed = WindowsStorable::try_parse("C:\\Windows").expect("C:\\Windows should parse");
        match parsed {
            WindowsStorableItem::Folder(folder) => {
                assert!(folder.id().eq_ignore_ascii_case("C:\\Windows"));
                assert!(!folder.name().is_empty());
            }
            WindowsStorableItem::File(_) => panic!("C:\\Windows should be a folder"),
        }
    }

    #[test]
    fn parse_missing_path_returns_none() {
        init_com();
        assert!(WindowsStorable::try_parse("C:\\this\\path\\does\\not\\exist\\hopefully-42").is_none());
    }

    #[test]
    fn parse_known_file_is_file() {
        init_com();
        let parsed =
            WindowsStorable::try_parse("C:\\Windows\\System32\\ntdll.dll").expect("ntdll should parse");
        assert!(matches!(parsed, WindowsStorableItem::File(_)));
    }

    #[test]
    fn get_parent_of_system32_is_windows() {
        init_com();
        let Some(WindowsStorableItem::Folder(folder)) =
            WindowsStorable::try_parse("C:\\Windows\\System32")
        else {
            panic!("System32 should parse as a folder");
        };
        let parent = folder.parent().expect("System32 has a parent");
        assert!(parent.id().eq_ignore_ascii_case("C:\\Windows"));
    }

    #[test]
    fn equality_compares_shell_identity() {
        init_com();
        let a = WindowsStorable::try_parse("C:\\Windows").unwrap();
        let b = WindowsStorable::try_parse("c:\\windows").unwrap();
        let c = WindowsStorable::try_parse("C:\\Windows\\System32").unwrap();
        assert_eq!(a.storable(), b.storable());
        assert_ne!(a.storable(), c.storable());
    }

    #[test]
    fn enumerate_temp_folder() {
        init_com();
        let dir = std::env::temp_dir().join(format!("drive-app-storage-test-{}", std::process::id()));
        let sub = dir.join("subdir");
        let file = dir.join("file.txt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(&file, b"hello").unwrap();

        let Some(WindowsStorableItem::Folder(folder)) =
            WindowsStorable::try_parse(dir.to_str().unwrap())
        else {
            panic!("temp folder should parse");
        };

        let all = folder.enumerate_items(StorableKind::ALL);
        assert_eq!(all.len(), 2);

        let files = folder.enumerate_items(StorableKind::FILES);
        assert_eq!(files.len(), 1);
        assert!(matches!(files[0], WindowsStorableItem::File(_)));

        let folders = folder.enumerate_items(StorableKind::FOLDERS);
        assert_eq!(folders.len(), 1);
        assert!(matches!(folders[0], WindowsStorableItem::Folder(_)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn folder_trait_streams_items() {
        use futures::StreamExt;

        init_com();
        let Some(WindowsStorableItem::Folder(folder)) = WindowsStorable::try_parse("C:\\Windows")
        else {
            panic!("C:\\Windows should parse as a folder");
        };
        let count = Folder::get_items(&folder, StorableKind::ALL).count().await;
        assert!(count > 0, "C:\\Windows should not be empty");
    }
}
