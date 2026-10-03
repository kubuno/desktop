//! Port of `Legacy/HomeFolder/HomeFolder.cs` (`IHomeFolder`).
//!
//! Aggregates Quick Access folders, logical drives and network locations into
//! a single virtual "Home" folder.

use futures::stream::{self, BoxStream, StreamExt};

use drive_core_storage::{Folder, Storable, StorableItem, StorableKind, StorageResult};

use crate::guids::{FOLDERID_NETHOOD, FOLDERID_QUICK_ACCESS, FOLDERID_RECENT};
use crate::windows_storage::{WindowsFolder, WindowsStorable, WindowsStorableItem};

/// Port of `HomeFolder : IHomeFolder`.
#[derive(Debug, Default)]
pub struct HomeFolder;

impl HomeFolder {
    pub fn new() -> Self {
        Self
    }

    /// Port of `GetQuickAccessFolderAsync` — enumerates the Quick Access
    /// virtual folder (`3936e9e4-d92c-4eee-a85a-bc16d5ea0819`).
    pub fn get_quick_access_folders(&self) -> Vec<WindowsStorableItem> {
        match WindowsFolder::from_known_folder(FOLDERID_QUICK_ACCESS) {
            Ok(folder) => folder.enumerate_items(StorableKind::FOLDERS),
            Err(error) => {
                tracing::warn!(?error, "failed to open the Quick Access folder");
                Vec::new()
            }
        }
    }

    /// Port of `GetLogicalDrivesAsync` — `GetLogicalDrives` bitmask → one
    /// [`WindowsFolder`] per drive root.
    pub fn get_logical_drives(&self) -> Vec<WindowsStorableItem> {
        // SAFETY: plain Win32 call.
        let available_drives = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
        if available_drives == 0 {
            return Vec::new();
        }

        let mut drives = Vec::new();
        for index in 0..26u32 {
            if available_drives & (1 << index) == 0 {
                continue;
            }
            let letter = (b'A' + index as u8) as char;
            let root = format!("{letter}:\\");
            match WindowsStorable::try_parse(&root) {
                Some(WindowsStorableItem::Folder(folder)) => {
                    drives.push(WindowsStorableItem::Folder(folder));
                }
                Some(WindowsStorableItem::File(file)) => {
                    // The C# code rewraps the parsed item as a folder
                    // unconditionally; a drive root is always a folder.
                    drives.push(WindowsStorableItem::Folder(WindowsFolder::new(
                        file.shell_item().clone(),
                    )));
                }
                None => tracing::warn!(%root, "failed to parse drive root"),
            }
        }

        drives
    }

    /// Port of `GetNetworkLocationsAsync` — enumerates `FOLDERID_NetHood`.
    pub fn get_network_locations(&self) -> Vec<WindowsStorableItem> {
        match WindowsFolder::from_known_folder(FOLDERID_NETHOOD) {
            Ok(folder) => folder.enumerate_items(StorableKind::FOLDERS),
            Err(error) => {
                tracing::warn!(?error, "failed to open the NetHood folder");
                Vec::new()
            }
        }
    }

    /// Port of `GetRecentFilesAsync` — enumerates `FOLDERID_Recent`.
    ///
    /// NOTE: the upstream C# code enumerates with `StorableType.Folder`; that
    /// filter is preserved for the 1:1 port.
    pub fn get_recent_files(&self) -> Vec<WindowsStorableItem> {
        match WindowsFolder::from_known_folder(FOLDERID_RECENT) {
            Ok(folder) => folder.enumerate_items(StorableKind::FOLDERS),
            Err(error) => {
                tracing::warn!(?error, "failed to open the Recent folder");
                Vec::new()
            }
        }
    }
}

impl Storable for HomeFolder {
    /// `"Home"` — will be `"files://Home"` in the future (same note as C#).
    fn id(&self) -> &str {
        "Home"
    }

    fn name(&self) -> &str {
        "Home"
    }

    fn as_storable(&self) -> &dyn Storable {
        self
    }
}

impl Folder for HomeFolder {
    /// Port of `GetItemsAsync` — Quick Access folders, then logical drives,
    /// then network locations (recent files are not part of the aggregate,
    /// same as C#).
    fn get_items(&self, _kind: StorableKind) -> BoxStream<'_, StorageResult<StorableItem>> {
        let mut items = self.get_quick_access_folders();
        items.extend(self.get_logical_drives());
        items.extend(self.get_network_locations());

        stream::iter(items.into_iter().map(|item| Ok(item.into_storable_item()))).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::init_com;

    #[test]
    fn logical_drives_contains_system_drive() {
        init_com();
        let drives = HomeFolder::new().get_logical_drives();
        assert!(!drives.is_empty(), "at least one logical drive should exist");
        assert!(
            drives.iter().any(|drive| drive.storable().id().to_ascii_uppercase().starts_with("C:")),
            "the C: drive should be listed"
        );
        assert!(drives.iter().all(|drive| matches!(drive, WindowsStorableItem::Folder(_))));
    }

    #[test]
    fn home_folder_identity() {
        let home = HomeFolder::new();
        assert_eq!(home.id(), "Home");
        assert_eq!(home.name(), "Home");
    }

    #[tokio::test]
    async fn home_folder_streams_at_least_the_drives() {
        use futures::StreamExt;

        init_com();
        let home = HomeFolder::new();
        let count = home.get_items(StorableKind::ALL).count().await;
        assert!(count > 0, "home folder aggregate should not be empty");
    }
}
