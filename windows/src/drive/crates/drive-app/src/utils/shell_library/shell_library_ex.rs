//! ShellLibraryEx (mirrors ShellLibraryEx.cs)
//!
//! Port of `Files.App/Utils/Shell/ShellLibraryEx.cs`: wraps `IShellLibrary`
//! (`LoadLibraryFromItem` in `STGM_READ`, `GetFolders(LFF_ALLITEMS)`,
//! `GetDefaultSaveFolder(DSFT_DETECT)`, `GetOptions() & LOF_PINNEDTONAVPANE`).
//! NOTE: the C# counterpart lives under `Utils/Shell/`; a strict mirror
//! would place it in `utils/shell/shell_library_ex.rs`. It stays here to
//! keep the `shell_library` module cohesive. `read_library` also MERGES
//! `ShellFolderExtensions.GetShellLibraryItem` (the C# separation isn't
//! clean-cut).
//!
//! Any COM error results in the library being ignored: never a panic.
//! Everything runs on the current STA thread (no new thread, no
//! `CoInitialize` — the model follows `utils/recycle_bin.rs`).

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ,
};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemArray, IShellLibrary, SHCreateItemFromParsingName,
    SHGetKnownFolderPath, ShellLibrary, DSFT_DETECT, FOLDERID_Libraries, KF_FLAG_DEFAULT,
    LFF_ALLITEMS, LOF_PINNEDTONAVPANE, SIGDN_FILESYSPATH,
};

use super::shell_library_item::LibraryInfo;

/// Retrieves the file system path of an `IShellItem` (`SIGDN_FILESYSPATH`),
/// freeing the `PWSTR` allocated by the shell.
fn item_filesystem_path(item: &IShellItem) -> Option<String> {
    unsafe {
        let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.0 as _));
        text.filter(|s| !s.is_empty())
    }
}

/// `ShellLibraryItem.LibrariesPath`: the libraries folder via
/// `SHGetKnownFolderPath(FOLDERID_Libraries)`
/// (`{1B3EA5DC-B587-4786-B4EF-BD1DC332AEAE}`).
pub(super) fn libraries_folder() -> Option<String> {
    unsafe {
        let pwstr: PWSTR =
            SHGetKnownFolderPath(&FOLDERID_Libraries, KF_FLAG_DEFAULT, None).ok()?;
        let text = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.0 as _));
        text.filter(|s| !s.is_empty())
    }
}

/// Opens a library read-only and extracts a `LibraryInfo` from it.
///
/// Mirrors `ShellLibraryEx(GetShellItemForPath(libFile), true)` followed
/// by `GetShellLibraryItem`. Any COM error = `None` (library ignored).
pub(super) fn read_library(full_path: &str) -> Option<LibraryInfo> {
    unsafe {
        // `Shell32.ShellUtil.GetShellItemForPath(libFile)`.
        let wide: Vec<u16> =
            full_path.encode_utf16().chain(std::iter::once(0)).collect();
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;

        // `new Shell32.IShellLibrary()` + `LoadLibraryFromItem(item, STGM_READ)`.
        let library: IShellLibrary =
            CoCreateInstance(&ShellLibrary, None, CLSCTX_ALL).ok()?;
        library.LoadLibraryFromItem(&item, STGM_READ.0).ok()?;

        // `PinnedToNavigationPane = GetOptions().IsFlagSet(LOF_PINNEDTONAVPANE)`.
        let is_pinned = library
            .GetOptions()
            .map(|o| (o.0 & LOF_PINNEDTONAVPANE.0) != 0)
            .unwrap_or(false);

        // `GetShellLibraryItem`: `DefaultSaveFolder`/`Folders` are only
        // populated if `folders.Count > 0`.
        let mut folders = Vec::new();
        if let Ok(array) = library.GetFolders::<IShellItemArray>(LFF_ALLITEMS) {
            if let Ok(count) = array.GetCount() {
                for i in 0..count {
                    if let Ok(folder) = array.GetItemAt(i) {
                        if let Some(path) = item_filesystem_path(&folder) {
                            folders.push(path);
                        }
                    }
                }
            }
        }

        let default_save_folder = if folders.is_empty() {
            None
        } else {
            library
                .GetDefaultSaveFolder::<IShellItem>(DSFT_DETECT)
                .ok()
                .as_ref()
                .and_then(item_filesystem_path)
        };

        // The name = file name without `.library-ms`.
        let name = std::path::Path::new(full_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        Some(LibraryInfo {
            full_path: full_path.to_string(),
            name,
            is_pinned,
            default_save_folder,
            folders,
        })
    }
}
