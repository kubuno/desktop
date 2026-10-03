//! HomeModel (port-only — aggregate of the Home page and sidebar data)

use super::{load_drives, load_quick_access, load_recent_files};
use super::{DriveItem, QuickAccessItem, RecentFileItem};

/// Home page + sidebar data.
pub struct HomeModel {
    pub quick_access: Vec<QuickAccessItem>,
    pub drives: Vec<DriveItem>,
    /// `CloudDrivesManager`: cloud providers detected via the registry.
    pub cloud_drives: Vec<crate::utils::cloud::CloudProvider>,
    /// Windows libraries (`LibraryManager.ListUserLibraries`).
    pub libraries: Vec<crate::utils::shell_library::LibraryInfo>,
    pub recent_files: Vec<RecentFileItem>,
}

impl HomeModel {
    pub fn load() -> Self {
        Self {
            quick_access: load_quick_access(),
            drives: load_drives(),
            cloud_drives: crate::utils::cloud::detect_cloud_drives(),
            libraries: if crate::services::settings::get().show_library_section {
                crate::utils::shell_library::list_libraries()
            } else {
                Vec::new()
            },
            recent_files: load_recent_files(8),
        }
    }
}
