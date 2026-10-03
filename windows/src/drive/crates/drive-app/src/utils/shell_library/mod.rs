//! READ-only port of the Windows libraries model.
//!
//! C# sources:
//! - `Files.App/Utils/Library/LibraryManager.cs` (`library_manager`).
//! - `Files.App/Utils/Shell/ShellLibraryEx.cs` (`shell_library_ex`) +
//!   `Utils/Shell/ShellFolderExtensions.cs` (`GetShellLibraryItem`, merged).
//! - `Files.App/Data/Items/ShellLibraryItem.cs` (`shell_library_item`) +
//!   `Data/Items/SidebarLibraryItem.cs` (`IsEmpty`).

pub mod library_manager;
pub mod shell_library_ex;
pub mod shell_library_item;

pub use library_manager::*;
pub use shell_library_item::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_library_path_reconnait_extension() {
        assert!(is_library_path("C:\\x\\Documents.library-ms"));
        assert!(is_library_path("Documents.LIBRARY-MS"));
        assert!(!is_library_path("C:\\x\\Documents.txt"));
        assert!(!is_library_path("library-ms"));
        assert!(!is_library_path(""));
    }

    #[test]
    fn is_empty_suit_le_csharp() {
        let vide = LibraryInfo {
            full_path: String::new(),
            name: String::new(),
            is_pinned: false,
            default_save_folder: None,
            folders: vec!["C:\\Docs".into()],
        };
        assert!(vide.is_empty());

        let plein = LibraryInfo {
            full_path: String::new(),
            name: String::new(),
            is_pinned: false,
            default_save_folder: Some("C:\\Docs".into()),
            folders: vec!["C:\\Docs".into()],
        };
        assert!(!plein.is_empty());
    }

    #[test]
    fn list_libraries_ne_panique_pas() {
        // On the current STA thread: must never panic, even without COM
        // explicitly initialized.
        let _ = list_libraries();
    }
}
