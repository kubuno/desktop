//! Port of `Files.App/Services/Windows/WindowsJumpListService.cs`: the
//! taskbar JUMPLIST (custom categories "Recent" and
//! "Pinned items").
//!
//! The original relies on the WinRT API `Windows.UI.StartScreen.JumpList`
//! (reserved for applications with a package identity). This port is pure
//! Win32: it uses the native COM equivalent `ICustomDestinationList`
//! (+ `IObjectCollection`/`IObjectArray` + `IShellLinkW`), exactly the API
//! the shell exposes to desktop applications.
//!
//! Structural difference from the C#: `ICustomDestinationList` requires a
//! full transaction (`BeginList` → `AppendCategory(…)` → `CommitList`);
//! a single item cannot be removed/added like
//! `instance.Items.Remove/Insert` does. The list is therefore REBUILT in full on
//! every call, from two static caches (recent + pinned).
//!
//! Robustness: any COM error is a silent no-op — never a panic.

use std::sync::Mutex;

use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, PSGetPropertyKeyFromName,
};
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, SHStrDupW,
    ShellLink,
};

/// Maximum number of folders kept in the "Recent" category
/// (the API only shows a handful of items per category anyway).
const MAX_RECENT: usize = 10;

/// Cache of recent folders: head = most recent, deduplicated, capped.
/// Mirrors the C#'s `instance.Items.Insert(0, …)` behavior (the most
/// recent stay on top), but on the port side since the list must be rebuilt
/// in full on every `CommitList`.
static RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Cache of pinned Quick Access folders, in shell order.
static PINNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `AddFolderAsync`: adds `path` to the top of the custom "Recent" category
/// (`Strings.JumpListRecentGroupHeader`), then rebuilds the complete
/// jumplist.
///
/// Virtual locations (Home, Settings, Recycle Bin, shell paths)
/// are ignored: these are targets without a file-system path
/// reusable as a command-line argument. Primary exclusion happens
/// at the call site (see report, hook a); this guard is an extra
/// safeguard.
pub fn add_recent_folder(path: &str) {
    if is_excluded(path) {
        return;
    }

    {
        let Ok(mut recent) = RECENT.lock() else {
            return;
        };
        // "Keep newer items at the top": remove any duplicate
        // (case-insensitive) then re-insert at the head.
        recent.retain(|p| !p.eq_ignore_ascii_case(path));
        recent.insert(0, path.to_string());
        recent.truncate(MAX_RECENT);
    }

    rebuild();
}

/// `RefreshPinnedFoldersAsync`: (re)sets the "Pinned items" category
/// (`Strings.JumpListPinnedGroupHeader`) with the given pinned Quick Access
/// folders, then rebuilds the complete jumplist.
pub fn refresh_pinned(paths: &[String]) {
    {
        let Ok(mut pinned) = PINNED.lock() else {
            return;
        };
        *pinned = paths
            .iter()
            .filter(|p| !is_excluded(p))
            .cloned()
            .collect();
    }

    rebuild();
}

/// Determines whether a path is a virtual location to exclude from the
/// jumplist (no real file-system path to pass as an argument).
fn is_excluded(path: &str) -> bool {
    let p = path.trim();
    // Empty, or shell pseudo-paths (`shell:…`, `::{CLSID}`): Home,
    // Settings, Recycle Bin, This PC, etc.
    p.is_empty() || p.starts_with("shell:") || p.starts_with("::")
}

/// Fully rebuilds the jumplist from the two caches.
///
/// `ICustomDestinationList` requires the full transaction; the order of the
/// `AppendCategory` calls sets the display order (first category = on top) —
/// so "Recent" is placed above "Pinned items", like the
/// original's `Insert(0, …)`.
fn rebuild() {
    // Cache snapshots (locks released before the COM calls).
    let recent: Vec<String> = RECENT.lock().map(|g| g.clone()).unwrap_or_default();
    let pinned: Vec<String> = PINNED.lock().map(|g| g.clone()).unwrap_or_default();

    // Any COM error = silent no-op.
    let _ = rebuild_inner(&recent, &pinned);
}

/// COM core of the rebuild. `STA` required: the UI thread already initializes
/// COM in a single-threaded apartment (`CoInitializeEx(COINIT_APARTMENTTHREADED)`
/// in `main`). We don't re-initialize here to avoid changing apartment.
fn rebuild_inner(recent: &[String], pinned: &[String]) -> windows::core::Result<()> {
    unsafe {
        // Each shortcut's `SetPath` points to the current executable;
        // the argument carries the folder path (like the C# passes the
        // path as the `JumpListItem`'s `Arguments`).
        let exe: HSTRING = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(HSTRING::from))
            .ok_or_else(windows::core::Error::from_thread)?;

        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;

        // `BeginList` opens the transaction and returns the targets removed by
        // the user (ignored here) via an `IObjectArray`.
        let mut _min_slots: u32 = 0;
        let _removed: IObjectArray = list.BeginList(&mut _min_slots)?;

        // "Recent" category first (so on top), then "Pinned items".
        if !recent.is_empty() {
            if let Some(array) = build_category(&exe, recent) {
                let header = HSTRING::from(header_recent());
                let _ = list.AppendCategory(&header, &array);
            }
        }
        if !pinned.is_empty() {
            if let Some(array) = build_category(&exe, pinned) {
                let header = HSTRING::from(header_pinned());
                let _ = list.AppendCategory(&header, &array);
            }
        }

        list.CommitList()?;
        Ok(())
    }
}

/// Builds the `IObjectArray` of `IShellLinkW` for a category.
unsafe fn build_category(exe: &HSTRING, paths: &[String]) -> Option<IObjectArray> {
    let collection: IObjectCollection =
        CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER).ok()?;

    for path in paths {
        if let Some(link) = build_shell_link(exe, path) {
            // An invalid link doesn't interrupt the whole category.
            let _ = collection.AddObject(&link);
        }
    }

    // `IObjectCollection` derives from `IObjectArray`: direct cast.
    collection.cast::<IObjectArray>().ok()
}

/// Builds an `IShellLinkW` for a folder (equivalent of the C#'s
/// `JumpListItem`): `SetPath(exe)`, `SetArguments(path)`, `SetDescription(path)`
/// and a visible title via `PKEY_Title` (`IPropertyStore`).
unsafe fn build_shell_link(exe: &HSTRING, path: &str) -> Option<IShellLinkW> {
    // "Jumplist item argument can't end with a slash so append a character
    // that can't exist in a directory name": drive root `C:\` →
    // the argument gets a trailing `?` (like the C#).
    let argument = if path.ends_with('\\') {
        format!("{path}?")
    } else {
        path.to_string()
    };
    let arg_h = HSTRING::from(argument.as_str());

    // Displayed title: folder name; for a drive root (no final
    // component) we fall back to the path itself ("C:\").
    // TODO(parity): the C# resolves localized names here (Desktop,
    // Downloads, Network, libraries…). Not ported: to be wired to the
    // future shell display service.
    let title = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(path);

    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    link.SetPath(exe).ok()?;
    link.SetArguments(&arg_h).ok()?;
    // `jumplistItem.Description = jumplistItem.Arguments`: the tooltip
    // reuses the argument (the path).
    link.SetDescription(&arg_h).ok()?;

    // Title via the link's `IPropertyStore` (System.Title / PKEY_Title).
    let _ = set_title(&link, title);

    Some(link)
}

/// Writes the shortcut's visible title into its `IPropertyStore`
/// (`PKEY_Title` = `System.Title`). Since `PKEY_Title` isn't exposed as an
/// identifier by windows-rs 0.62, the key is resolved by name via `PSGetPropertyKeyFromName`.
unsafe fn set_title(link: &IShellLinkW, title: &str) -> windows::core::Result<()> {
    let store: IPropertyStore = link.cast()?;

    let mut key: PROPERTYKEY = Default::default();
    PSGetPropertyKeyFromName(windows::core::w!("System.Title"), &mut key)?;

    // PROPVARIANT of type VT_LPWSTR: the string is allocated by the shell
    // (`SHStrDupW` → `CoTaskMemAlloc`) and freed afterward by
    // `PropVariantClear`.
    let title_h = HSTRING::from(title);
    let pwsz: PWSTR = SHStrDupW(&title_h)?;

    let mut pv = PROPVARIANT::default();
    {
        let inner = &mut *pv.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = pwsz;
    }

    let result = store.SetValue(&key, &pv).and_then(|_| store.Commit());

    // Free the string owned by the PROPVARIANT (VT_LPWSTR).
    let _ = PropVariantClear(&mut pv);

    result
}

/// Header for the "Recent" category (`Strings.JumpListRecentGroupHeader`).
fn header_recent() -> String {
    drive_localization::tr("JumpListRecentGroupHeader").to_string()
}

/// Header for the "Pinned items" category
/// (`Strings.JumpListPinnedGroupHeader`).
fn header_pinned() -> String {
    drive_localization::tr("JumpListPinnedGroupHeader").to_string()
}
