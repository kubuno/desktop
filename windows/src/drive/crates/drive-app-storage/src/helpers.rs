//! Port of `Windows/Helpers/WindowsStorableHelpers.Shell.cs` and
//! `WindowsStorableHelpers.Storage.cs` (plus the non-bitmap parts of
//! `WindowsStorableHelpers.Icon.cs`).
//!
//! The C# extension methods on `IWindowsStorable`/`IWindowsFolder` become
//! inherent methods on [`WindowsStorable`]/[`WindowsFolder`].
//!
//! Interop layers return `windows::core::Result`; conversions to
//! `StorageError` happen at the `drive-core-storage` trait boundaries.
//!
//! NOTE: interactive shell calls (`try_invoke_context_menu_verb`,
//! `get_shell_new_items`, pin/unpin, …) require an STA thread; run them via
//! [`crate::sta_thread`].

use windows::core::{Error, Interface, HSTRING, PCSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, PROPERTYKEY};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileAttributesW, SetFileAttributesW, COMPRESSION_FORMAT_DEFAULT,
    COMPRESSION_FORMAT_NONE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, INVALID_FILE_ATTRIBUTES,
    OPEN_EXISTING,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::SystemServices::SFGAO_FLAGS;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::FSCTL_SET_COMPRESSION;
use windows::Win32::UI::Shell::{
    IContextMenu, IContextMenu2, IExecuteCommand, IObjectWithSelection, IQueryInfo, IShellExtInit,
    IShellItem2, IShellItemArray, IShellLinkW, PropertiesSystem::PSGetPropertyKeyFromName,
    SHCreateShellItemArrayFromShellItem, SHFormatDrive, SHGetIDListFromObject,
    SHGetSetFolderCustomSettings, BHID_SFUIObject, CMF_OPTIMIZEFORINVOKE, CMINVOKECOMMANDINFO,
    FCSM_ICONFILE, FCS_FORCEWRITE, QITIPF_DEFAULT, SHFMT_ID, SHFMT_OPT, SHFOLDERCUSTOMSETTINGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, GetSubMenu, MENUITEMINFOW,
    MIIM_ID, MIIM_STATE, MIIM_STRING, SW_HIDE, SW_SHOWNORMAL, WM_INITMENUPOPUP,
};

use crate::guids::{CLSID_NEW_MENU, CLSID_PIN_TO_FREQUENT_EXECUTE, CLSID_UNPIN_FROM_FREQUENT_EXECUTE};
use crate::windows_storage::{take_co_task_string, WindowsFolder, WindowsStorable};

/// Port of `WindowsContextMenuType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowsContextMenuType {
    Normal = 0x0000_0000,
    Disabled = 0x0000_0003,
    Checked = 0x0000_0008,
    Highlighted = 0x0000_0080,
    Default = 0x0000_1000,
}

impl From<u32> for WindowsContextMenuType {
    fn from(state: u32) -> Self {
        match state {
            0x0000_0003 => Self::Disabled,
            0x0000_0008 => Self::Checked,
            0x0000_0080 => Self::Highlighted,
            0x0000_1000 => Self::Default,
            _ => Self::Normal,
        }
    }
}

/// Port of `WindowsContextMenuItem`.
#[derive(Debug, Clone)]
pub struct WindowsContextMenuItem {
    pub menu_type: WindowsContextMenuType,
    pub id: u32,
    pub icon: Option<Vec<u8>>,
    pub name: Option<String>,
}

impl WindowsStorable {
    /// Port of `GetPropertyValue<string>` — `IShellItem2::GetString` with a
    /// property key resolved via `PSGetPropertyKeyFromName`.
    pub fn get_property_string(&self, prop_key: &str) -> windows::core::Result<String> {
        // SAFETY: standard property-store access; the returned string is
        // released via `take_co_task_string`.
        unsafe {
            let item2: IShellItem2 = self.shell_item().cast()?;
            let mut key = PROPERTYKEY::default();
            PSGetPropertyKeyFromName(&HSTRING::from(prop_key), &mut key)?;
            let value = item2.GetString(&key)?;
            Ok(take_co_task_string(value))
        }
    }

    /// Port of `GetPropertyValue<bool>` — `IShellItem2::GetBool`.
    pub fn get_property_bool(&self, prop_key: &str) -> windows::core::Result<bool> {
        // SAFETY: standard property-store access.
        unsafe {
            let item2: IShellItem2 = self.shell_item().cast()?;
            let mut key = PROPERTYKEY::default();
            PSGetPropertyKeyFromName(&HSTRING::from(prop_key), &mut key)?;
            Ok(item2.GetBool(&key)?.as_bool())
        }
    }

    /// Port of `HasShellAttributes`.
    pub fn has_shell_attributes(&self, attributes: SFGAO_FLAGS) -> bool {
        // SAFETY: `self` owns a live shell item.
        unsafe {
            self.shell_item()
                .GetAttributes(attributes)
                .map(|returned| returned == attributes)
                .unwrap_or(false)
        }
    }

    /// Port of `TryInvokeContextMenuVerb`. Requires an STA thread.
    pub fn try_invoke_context_menu_verb(&self, verb_name: &str) -> windows::core::Result<()> {
        self.try_invoke_context_menu_verbs(&[verb_name], false)
    }

    /// Port of `TryInvokeContextMenuVerbs`. Requires an STA thread.
    pub fn try_invoke_context_menu_verbs(
        &self,
        verb_names: &[&str],
        early_return_on_success: bool,
    ) -> windows::core::Result<()> {
        // SAFETY: standard context-menu invocation; the popup menu is
        // destroyed on every exit path.
        unsafe {
            let context_menu: IContextMenu =
                self.shell_item().BindToHandler(None, &BHID_SFUIObject)?;
            let hmenu = CreatePopupMenu()?;
            let _hr = context_menu.QueryContextMenu(hmenu, 0, 1, 0x7FFF, CMF_OPTIMIZEFORINVOKE);

            let mut last: windows::core::Result<()> = Ok(());
            for verb_name in verb_names {
                // The verb must be an ASCII/ANSI NUL-terminated string.
                let mut verb: Vec<u8> = verb_name.bytes().collect();
                verb.push(0);

                let mut cmici = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    lpVerb: PCSTR(verb.as_ptr()),
                    nShow: SW_HIDE.0,
                    ..Default::default()
                };
                last = context_menu.InvokeCommand(&mut cmici as *mut _ as *const _);

                if last.is_ok() && early_return_on_success {
                    break;
                }
            }

            DestroyMenu(hmenu)?;
            last
        }
    }

    /// Port of `TryGetShellTooltip` — `IQueryInfo::GetInfoTip`.
    pub fn try_get_shell_tooltip(&self) -> windows::core::Result<String> {
        // SAFETY: standard tooltip query; the returned string is released via
        // `take_co_task_string`.
        unsafe {
            let query_info: IQueryInfo = self.shell_item().BindToHandler(None, &BHID_SFUIObject)?;
            let tip = query_info.GetInfoTip(QITIPF_DEFAULT)?;
            Ok(take_co_task_string(tip))
        }
    }

    // -- WindowsStorableHelpers.Storage.cs ---------------------------------

    /// Port of `TryGetFileAttributes`.
    pub fn try_get_file_attributes(&self) -> Option<FILE_FLAGS_AND_ATTRIBUTES> {
        // SAFETY: plain Win32 call on an owned path string.
        let attributes = unsafe { GetFileAttributesW(&HSTRING::from(self.id())) };
        if attributes == INVALID_FILE_ATTRIBUTES {
            None
        } else {
            Some(FILE_FLAGS_AND_ATTRIBUTES(attributes))
        }
    }

    /// Port of `TrySetFileAttributes`.
    pub fn try_set_file_attributes(&self, attributes: FILE_FLAGS_AND_ATTRIBUTES) -> bool {
        if attributes == FILE_ATTRIBUTE_COMPRESSED {
            return self.try_toggle_file_compressed_attribute(true);
        }

        let Some(previous) = self.try_get_file_attributes() else {
            return false;
        };
        // SAFETY: plain Win32 call.
        unsafe { SetFileAttributesW(&HSTRING::from(self.id()), previous | attributes).is_ok() }
    }

    /// Port of `TryUnsetFileAttributes`.
    pub fn try_unset_file_attributes(&self, attributes: FILE_FLAGS_AND_ATTRIBUTES) -> bool {
        if attributes == FILE_ATTRIBUTE_COMPRESSED {
            return self.try_toggle_file_compressed_attribute(false);
        }

        let Some(previous) = self.try_get_file_attributes() else {
            return false;
        };
        // SAFETY: plain Win32 call.
        unsafe {
            SetFileAttributesW(&HSTRING::from(self.id()), previous & !attributes).is_ok()
        }
    }

    /// Port of `TryToggleFileCompressedAttribute` — `DeviceIoControl` with
    /// `FSCTL_SET_COMPRESSION`.
    pub fn try_toggle_file_compressed_attribute(&self, value: bool) -> bool {
        // SAFETY: the handle is validated by `CreateFileW` returning `Ok` and
        // closed on every exit path.
        unsafe {
            // GENERIC_READ | GENERIC_WRITE flags are needed here;
            // FILE_FLAG_BACKUP_SEMANTICS is used to open directories.
            let Ok(handle) = CreateFileW(
                &HSTRING::from(self.id()),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_WRITE_ATTRIBUTES).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            ) else {
                return false;
            };

            let compression_format = if value {
                COMPRESSION_FORMAT_DEFAULT
            } else {
                COMPRESSION_FORMAT_NONE
            };

            let mut bytes_returned = 0u32;
            let result = DeviceIoControl(
                handle,
                FSCTL_SET_COMPRESSION,
                Some(&compression_format as *const _ as *const _),
                std::mem::size_of::<u16>() as u32,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            );

            let _ = CloseHandle(handle);
            result.is_ok()
        }
    }

    // -- Non-bitmap parts of WindowsStorableHelpers.Icon.cs ----------------

    /// Port of `TrySetFolderIcon` — `SHGetSetFolderCustomSettings` with
    /// `FCSM_ICONFILE`.
    pub fn try_set_folder_icon(
        &self,
        icon_file: &WindowsStorable,
        index: i32,
    ) -> windows::core::Result<()> {
        let folder_path = HSTRING::from(self.id());
        let mut icon_path: Vec<u16> = icon_file.id().encode_utf16().chain([0]).collect();

        let mut settings = SHFOLDERCUSTOMSETTINGS {
            dwSize: std::mem::size_of::<SHFOLDERCUSTOMSETTINGS>() as u32,
            dwMask: FCSM_ICONFILE,
            pszIconFile: PWSTR(icon_path.as_mut_ptr()),
            cchIconFile: 0,
            iIconIndex: index,
            ..Default::default()
        };

        // SAFETY: `settings` and both strings outlive the call.
        unsafe { SHGetSetFolderCustomSettings(&mut settings, &folder_path, FCS_FORCEWRITE) }
    }

    /// Port of `TrySetShortcutIcon` — `IShellLinkW::SetIconLocation`.
    pub fn try_set_shortcut_icon(
        &self,
        icon_file: &WindowsStorable,
        index: i32,
    ) -> windows::core::Result<()> {
        // SAFETY: standard shell-link access.
        unsafe {
            let shell_link: IShellLinkW = self.shell_item().BindToHandler(None, &BHID_SFUIObject)?;
            shell_link.SetIconLocation(&HSTRING::from(icon_file.id()), index)
        }
    }
}

impl WindowsFolder {
    /// Port of `TryPinFolderToQuickAccess`.
    pub fn try_pin_to_quick_access(&self) -> windows::core::Result<()> {
        self.execute_frequent_verb(&CLSID_PIN_TO_FREQUENT_EXECUTE)
    }

    /// Port of `TryUnpinFolderFromQuickAccess`.
    pub fn try_unpin_from_quick_access(&self) -> windows::core::Result<()> {
        self.execute_frequent_verb(&CLSID_UNPIN_FROM_FREQUENT_EXECUTE)
    }

    /// Shared body of the pin/unpin helpers: `IExecuteCommand` +
    /// `IObjectWithSelection::SetSelection` + `Execute`.
    fn execute_frequent_verb(&self, clsid: &windows::core::GUID) -> windows::core::Result<()> {
        // SAFETY: standard COM activation and execution sequence.
        unsafe {
            let execute_command: IExecuteCommand =
                CoCreateInstance(clsid, None, CLSCTX_INPROC_SERVER)?;

            let item_array: IShellItemArray = SHCreateShellItemArrayFromShellItem(self.shell_item())?;

            let with_selection: IObjectWithSelection = execute_command.cast()?;
            with_selection.SetSelection(&item_array)?;

            execute_command.Execute()
        }
    }

    /// Port of `GetShellNewItems` — enumerates the entries of the shell "New"
    /// submenu (`CLSID_NewMenu`). Requires an STA thread.
    pub fn get_shell_new_items(&self) -> windows::core::Result<Vec<WindowsContextMenuItem>> {
        // SAFETY: the menu handles are destroyed before returning; the pidl is
        // freed via `CoTaskMemFree`; COM interfaces are reference counted.
        unsafe {
            let new_menu: IContextMenu = CoCreateInstance(&CLSID_NEW_MENU, None, CLSCTX_INPROC_SERVER)?;
            let context_menu2: IContextMenu2 = new_menu.cast()?;
            let shell_ext_init: IShellExtInit = new_menu.cast()?;

            self.set_shell_new_menu(Some(new_menu.clone()));

            let folder_pidl = SHGetIDListFromObject(self.shell_item())?;
            let init_result = shell_ext_init.Initialize(Some(folder_pidl), None, None);
            CoTaskMemFree(Some(folder_pidl as _));
            init_result?;

            // Inserts "New (&W)"
            let hmenu = CreatePopupMenu()?;
            let hr = new_menu.QueryContextMenu(hmenu, 0, 1, 256, 0);
            if hr.is_err() {
                let _ = DestroyMenu(hmenu);
                return Err(Error::from_hresult(hr));
            }

            // Invokes CNewMenu::_InitMenuPopup(), which populates the submenu.
            let sub_menu = GetSubMenu(hmenu, 0);
            if let Err(error) =
                context_menu2.HandleMenuMsg(WM_INITMENUPOPUP, windows::Win32::Foundation::WPARAM(sub_menu.0 as usize), windows::Win32::Foundation::LPARAM(0))
            {
                let _ = DestroyMenu(hmenu);
                return Err(error);
            }

            let count = GetMenuItemCount(Some(sub_menu));
            if count == -1 {
                let _ = DestroyMenu(hmenu);
                return Err(Error::from_hresult(E_FAIL));
            }

            // Enumerates and populates the list.
            let mut shell_new_items = Vec::new();
            for index in 0..count as u32 {
                let mut text = [0u16; 256];
                let mut mii = MENUITEMINFOW {
                    cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_STRING | MIIM_ID | MIIM_STATE,
                    dwTypeData: PWSTR(text.as_mut_ptr()),
                    cch: text.len() as u32,
                    ..Default::default()
                };

                if GetMenuItemInfoW(sub_menu, index, true, &mut mii).is_ok() {
                    let len = text.iter().position(|&c| c == 0).unwrap_or(text.len());
                    shell_new_items.push(WindowsContextMenuItem {
                        id: mii.wID,
                        name: Some(String::from_utf16_lossy(&text[..len])),
                        menu_type: WindowsContextMenuType::from(mii.fState.0),
                        icon: None,
                    });
                }
            }

            let _ = DestroyMenu(hmenu);
            Ok(shell_new_items)
        }
    }

    /// Port of `InvokeShellNewItem` — invokes an entry returned by
    /// [`Self::get_shell_new_items`] by its command id. Requires an STA thread.
    pub fn invoke_shell_new_item(&self, item: &WindowsContextMenuItem) -> windows::core::Result<()> {
        // SAFETY: standard context-menu invocation with a MAKEINTRESOURCE-style
        // verb (command id).
        unsafe {
            let new_menu = match self.shell_new_menu() {
                Some(menu) => menu,
                None => {
                    let menu: IContextMenu =
                        CoCreateInstance(&CLSID_NEW_MENU, None, CLSCTX_INPROC_SERVER)?;
                    self.set_shell_new_menu(Some(menu.clone()));
                    menu
                }
            };

            let cmici = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                lpVerb: PCSTR(item.id as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };

            new_menu.InvokeCommand(&cmici)
        }
    }
}

/// Port of `TryShowFormatDriveDialog`.
///
/// NOTE: this calls an undocumented elevatable COM class
/// (`shell32.dll!CFormatEngine`), so the caller does not need to be elevated.
pub fn try_show_format_drive_dialog(
    hwnd: windows::Win32::Foundation::HWND,
    drive_letter_index: u32,
    id: SHFMT_ID,
    options: SHFMT_OPT,
) -> bool {
    // SAFETY: plain shell dialog call.
    let result = unsafe { SHFormatDrive(hwnd, drive_letter_index, id, options) };
    result == 0xFFFF
}

/// Port of `TryRenameVolumeLabel`.
pub fn try_rename_volume_label(_path: &str, _new_label: &str) -> bool {
    // TODO: Use shell32.dll!CMountPointRename
    // (CLSID: 60173D16-A550-47f0-A14B-C6F9E4DA0831, IID: 92F8D886-AB61-4113-BD4F-2E894397386F)
    false
}

#[cfg(test)]
mod tests {
    use crate::tests_support::init_com;
    use crate::windows_storage::{WindowsStorable, WindowsStorableItem};
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
    use windows::Win32::System::SystemServices::SFGAO_FILESYSTEM;

    fn parse(path: &str) -> WindowsStorableItem {
        WindowsStorable::try_parse(path).expect("path should parse")
    }

    #[test]
    fn windows_dir_has_directory_attribute() {
        init_com();
        let item = parse("C:\\Windows");
        let attributes = item.storable().try_get_file_attributes().expect("attributes");
        assert!((attributes & FILE_ATTRIBUTE_DIRECTORY).0 != 0);
    }

    #[test]
    fn windows_dir_has_filesystem_shell_attribute() {
        init_com();
        let item = parse("C:\\Windows");
        assert!(item.storable().has_shell_attributes(SFGAO_FILESYSTEM));
    }

    #[test]
    fn item_name_property_matches() {
        init_com();
        let item = parse("C:\\Windows\\System32\\ntdll.dll");
        let name = item
            .storable()
            .get_property_string("System.FileName")
            .expect("System.FileName should resolve");
        assert!(name.eq_ignore_ascii_case("ntdll.dll"));
    }
}
