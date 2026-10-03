//! Port of `Files.App/Services/Windows/WindowsQuickAccessService.cs`: the
//! PINNED Quick Access folders, enumerated from the shell folder
//! "Frequent places" (`::{3936e9e4-d92c-4eee-a85a-bc16d5ea0819}` —
//! the Quick Access folder itself would also add recent files here),
//! filtered on the `System.Home.IsPinned` property.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
use windows::Win32::System::SystemServices::SFGAO_FOLDER;
use windows::Win32::UI::Shell::{
    IEnumShellItems, IShellItem, IShellItem2, SHCreateItemFromParsingName, BHID_EnumItems,
    SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
};

/// A pinned item: file-system path + shell display name.
pub struct PinnedFolder {
    pub path: String,
    pub name: String,
}

/// `GetPinnedFoldersAsync`: the pinned folders, in shell order.
/// `None` if the virtual folder is unavailable (the caller's fallback
/// then keeps the known folders).
pub fn pinned_folders() -> Option<Vec<PinnedFolder>> {
    unsafe {
        let frequent: IShellItem = SHCreateItemFromParsingName(
            windows::core::w!("shell:::{3936e9e4-d92c-4eee-a85a-bc16d5ea0819}"),
            None,
        )
        .ok()?;
        let enumerator: IEnumShellItems = frequent.BindToHandler(None, &BHID_EnumItems).ok()?;

        let mut pinned_key = Default::default();
        PSGetPropertyKeyFromName(windows::core::w!("System.Home.IsPinned"), &mut pinned_key)
            .ok()?;

        let mut out = Vec::new();
        loop {
            let mut items: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            if enumerator.Next(&mut items, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = items[0].take() else { break };

            // `.Where(link => link.IsFolder)`.
            let attrs = item.GetAttributes(SFGAO_FOLDER).unwrap_or_default();
            if (attrs.0 & SFGAO_FOLDER.0) == 0 {
                continue;
            }
            // `System.Home.IsPinned`: non-pinned items in the "frequent"
            // folder are discarded.
            let Ok(item2) = windows::core::Interface::cast::<IShellItem2>(&item) else {
                continue;
            };
            let pinned = item2.GetBool(&pinned_key).is_ok_and(|b| b.as_bool());
            if !pinned {
                continue;
            }

            let Some(path) = display_name(&item, SIGDN_FILESYSPATH) else {
                continue; // non-file (shell) locations, out of scope
            };
            let name = display_name(&item, SIGDN_NORMALDISPLAY).unwrap_or_else(|| path.clone());
            out.push(PinnedFolder { path, name });
        }
        Some(out)
    }
}

fn display_name(
    item: &IShellItem,
    kind: windows::Win32::UI::Shell::SIGDN,
) -> Option<String> {
    unsafe {
        let pwstr: PWSTR = item.GetDisplayName(kind).ok()?;
        let s = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.as_ptr() as *const _));
        s.filter(|s| !s.is_empty())
    }
}

// Avoids an unused import when PCWSTR is only used by the w! macros.
#[allow(unused)]
fn _pcwstr_used(_: PCWSTR) {}
