//! LibraryManager (mirrors LibraryManager.cs)
//!
//! Port of `Files.App/Utils/Library/LibraryManager.cs` (`ListUserLibraries`,
//! `IsLibraryPath`): enumerates `%APPDATA%\Microsoft\Windows\Libraries\*.library-ms`
//! and opens each file read-only. NOTE: the C# counterpart lives under
//! `Utils/Library/`; a strict mirror would place it in
//! `utils/library/library_manager.rs`. It stays here to preserve
//! `crate::utils::shell_library::{is_library_path, list_libraries}`.

use super::shell_library_ex::{libraries_folder, read_library};
use super::shell_library_item::{LibraryInfo, EXTENSION};

/// Port of `LibraryManager.IsLibraryPath`: a path ending in
/// `.library-ms` (case-insensitive).
pub fn is_library_path(path: &str) -> bool {
    path.len() >= EXTENSION.len()
        && path[path.len() - EXTENSION.len()..].eq_ignore_ascii_case(EXTENSION)
}

/// Port of `LibraryManager.ListUserLibraries`: enumerates
/// `%APPDATA%\Microsoft\Windows\Libraries\*.library-ms` and reads each
/// library. Unreadable files are simply skipped.
pub fn list_libraries() -> Vec<LibraryInfo> {
    let Some(folder) = libraries_folder() else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };

    let mut libraries = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(path_str) = path.to_str() else {
            continue;
        };
        if !is_library_path(path_str) {
            continue;
        }
        if let Some(info) = read_library(path_str) {
            libraries.push(info);
        }
    }
    libraries
}
