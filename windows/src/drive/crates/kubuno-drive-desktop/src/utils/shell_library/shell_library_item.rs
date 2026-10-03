//! ShellLibraryItem (mirrors ShellLibraryItem.cs)
//!
//! Port of `Files.App/Data/Items/ShellLibraryItem.cs` (+ `IsEmpty` from
//! `Data/Items/SidebarLibraryItem.cs` / `LibraryLocationItem`). NOTE: the
//! C# counterpart lives under `Data/Items/`; a strict mirror would place
//! it in `data/items/shell_library_item.rs` (outside the `utils/` area).
//! It stays here to preserve `crate::utils::shell_library::LibraryInfo`
//! (imported by `data/items.rs`).

/// `ShellLibraryItem` restricted to what reading exposes (Rust mirror of
/// the C# `ShellLibraryItem` + `LibraryLocationItem`). Some fields are only
/// read by the library editor, which is not ported yet.
#[allow(dead_code)]
pub struct LibraryInfo {
    /// Full path of the `*.library-ms` file.
    pub full_path: String,
    /// Display name (file name without `.library-ms`).
    pub name: String,
    /// `LOF_PINNEDTONAVPANE`: pinned to the navigation pane.
    pub is_pinned: bool,
    /// Default save folder (`DSFT_DETECT`), if any.
    pub default_save_folder: Option<String>,
    /// Paths of member folders (`LFF_ALLITEMS`).
    pub folders: Vec<String>,
}

impl LibraryInfo {
    /// Port of `LibraryLocationItem.IsEmpty`:
    /// `DefaultSaveFolder is null || Folders is null || Folders.Count is 0`.
    /// Used by the library editor, not ported yet.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.default_save_folder.is_none() || self.folders.is_empty()
    }
}

/// Port of `ShellLibraryItem.EXTENSION`.
pub(super) const EXTENSION: &str = ".library-ms";
