//! StorageTrashBinService (mirror of StorageTrashBinService.cs)
//!
//! Port of `Files.App/Services/Storage/StorageTrashBinService.cs`: the
//! Recycle Bin via the Shell — enumeration (`SHGetKnownFolderItem` +
//! `BHID_EnumItems`), restoration (`IFileOperation::MoveItem` to
//! `System.Recycle.DeletedFrom`), emptying (`SHEmptyRecycleBin`), permanent
//! deletion (`IFileOperation::DeleteItem`).

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::SystemServices::SFGAO_FOLDER;
use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
use windows::Win32::UI::Shell::{
    FileOperation, IEnumShellItems, IFileOperation, IShellItem, IShellItem2,
    SHCreateItemFromParsingName, SHEmptyRecycleBinW, SHGetKnownFolderItem,
    SHQueryRecycleBinW, BHID_EnumItems, FOLDERID_RecycleBinFolder, FOF_NO_UI,
    KF_FLAG_DEFAULT, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHQUERYRBINFO,
    SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
};

/// A Recycle Bin item (`ShellFileItem` restricted to what the view
/// displays): `path` is the real `$RECYCLE.BIN\$R…` path (icons, uniqueness).
pub struct TrashItem {
    pub name: String,
    pub path: String,
    pub original_path: String,
    pub date_deleted: Option<std::time::SystemTime>,
    pub size: u64,
    pub is_dir: bool,
}

fn property_key(name: &str) -> Option<PROPERTYKEY> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut key = PROPERTYKEY::default();
    unsafe { PSGetPropertyKeyFromName(PCWSTR(wide.as_ptr()), &mut key) }.ok()?;
    Some(key)
}

fn filetime_to_system(ft: windows::Win32::Foundation::FILETIME) -> std::time::SystemTime {
    let intervals = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    let unix_ns = intervals.saturating_sub(116_444_736_000_000_000) * 100;
    std::time::UNIX_EPOCH + std::time::Duration::from_nanos(unix_ns)
}

/// `IsUnderTrashBin` (`RegexHelpers.RecycleBinPath`): the physical path is
/// under an `X:\$Recycle.Bin\`.
pub fn is_under_trash_bin(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.len() > 3
        && lower.as_bytes()[1] == b':'
        && lower[2..].starts_with("\\$recycle.bin\\")
}

/// The Recycle Bin folder's `IEnumShellItems`.
fn enumerate_bin() -> windows::core::Result<IEnumShellItems> {
    unsafe {
        let folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)?;
        folder.BindToHandler(None, &BHID_EnumItems)
    }
}

/// `GetAllRecycleBinFoldersAsync`: all Recycle Bin items.
pub fn list() -> Vec<TrashItem> {
    let Ok(enumerator) = enumerate_bin() else {
        return Vec::new();
    };
    let Some(deleted_from) = property_key("System.Recycle.DeletedFrom") else {
        return Vec::new();
    };
    let date_deleted_key = property_key("System.Recycle.DateDeleted");

    let mut items = Vec::new();
    loop {
        let mut fetched = [None];
        if unsafe { enumerator.Next(&mut fetched, None) }.is_err() {
            break;
        }
        let Some(item) = fetched[0].take() else { break };
        let Ok(item2): Result<IShellItem2, _> = item.cast() else { continue };

        let display = |sigdn| unsafe {
            item.GetDisplayName(sigdn)
                .map(|p| {
                    let text = p.to_string().unwrap_or_default();
                    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                    text
                })
                .unwrap_or_default()
        };
        // The display name of a Recycle Bin item is its full ORIGINAL
        // PATH — the view shows only the name (`ShellFileItem
        // .FileName`), the original folder having its own column.
        let display_name = display(SIGDN_NORMALDISPLAY);
        let path = display(SIGDN_FILESYSPATH);
        if display_name.is_empty() || path.is_empty() {
            continue;
        }
        let name = std::path::Path::new(&display_name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(display_name);

        let original_dir = unsafe {
            item2
                .GetString(&deleted_from)
                .map(|p| {
                    let text = p.to_string().unwrap_or_default();
                    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                    text
                })
                .unwrap_or_default()
        };
        let date_deleted = date_deleted_key
            .and_then(|key| unsafe { item2.GetFileTime(&key) }.ok())
            .map(filetime_to_system);
        let size = unsafe { item2.GetUInt64(&property_key("System.Size").unwrap_or_default()) }
            .unwrap_or(0);
        let is_dir = unsafe { item.GetAttributes(SFGAO_FOLDER) }
            .map(|a| a.0 != 0)
            .unwrap_or(false);

        items.push(TrashItem {
            name,
            path,
            original_path: original_dir,
            date_deleted,
            size,
            is_dir,
        });
    }
    items
}

/// `(HasRecycleBin, NumItems, BinSize)` — `SHQueryRecycleBin`.
pub fn query() -> (bool, i64, i64) {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    match unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) } {
        Ok(()) => (true, info.i64NumItems, info.i64Size),
        Err(_) => (false, 0, 0),
    }
}

/// `EmptyTrashBin`: `SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI` (the
/// confirmation is the app's dialog, not the shell's).
pub fn empty() -> bool {
    
    unsafe {
        SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI)
    }
    .is_ok()
}

/// The common restore/delete skeleton: walks the Recycle Bin and
/// applies `op` to items whose `$R…` path is in `paths` (empty =
/// all), then executes the `IFileOperation`.
fn operate_on_items(
    paths: &[String],
    op: impl Fn(&IFileOperation, &IShellItem, &IShellItem2) -> windows::core::Result<()>,
) -> bool {
    let Ok(enumerator) = enumerate_bin() else {
        return false;
    };
    let Ok(operation): windows::core::Result<IFileOperation> =
        (unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) })
    else {
        return false;
    };
    // FOF_NO_UI = FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR.
    let _ = unsafe { operation.SetOperationFlags(FOF_NO_UI) };

    let mut any = false;
    loop {
        let mut fetched = [None];
        if unsafe { enumerator.Next(&mut fetched, None) }.is_err() {
            break;
        }
        let Some(item) = fetched[0].take() else { break };
        if !paths.is_empty() {
            let path = unsafe {
                item.GetDisplayName(SIGDN_FILESYSPATH)
                    .map(|p| {
                        let text = p.to_string().unwrap_or_default();
                        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                        text
                    })
                    .unwrap_or_default()
            };
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                continue;
            }
        }
        let Ok(item2): Result<IShellItem2, _> = item.cast() else { continue };
        if op(&operation, &item, &item2).is_ok() {
            any = true;
        }
    }
    if !any {
        return false;
    }
    
    unsafe { operation.PerformOperations() }.is_ok()
}

/// `RestoreAllTrashesInternal` (empty paths) / restoring a selection:
/// `MoveItem` of each item to its `System.Recycle.DeletedFrom`.
pub fn restore(paths: &[String]) -> bool {
    let Some(deleted_from) = property_key("System.Recycle.DeletedFrom") else {
        return false;
    };
    operate_on_items(paths, |operation, item, item2| unsafe {
        let original = item2.GetString(&deleted_from).map(|p| {
            let text = p.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
            text
        })?;
        let wide: Vec<u16> = original.encode_utf16().chain(std::iter::once(0)).collect();
        let dest: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
        operation.MoveItem(item, &dest, PCWSTR::null(), None)
    })
}

pub fn restore_all() -> bool {
    restore(&[])
}

/// PERMANENT deletion of Recycle Bin items (the `$I` follows the `$R`,
/// unlike a plain delete of the `$R…` file).
pub fn delete(paths: &[String]) -> bool {
    if paths.is_empty() {
        return false;
    }
    operate_on_items(paths, |operation, item, _| unsafe {
        operation.DeleteItem(item, None)
    })
}
