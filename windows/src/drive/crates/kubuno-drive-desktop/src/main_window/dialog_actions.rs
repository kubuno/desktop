#![allow(unused_imports)]
//! Submodule of `MainWindow` — see `main_window/mod.rs`.
use windows::core::{w, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::view_models::shell_view_model::{browsable, shell_open, Location, TabGroup};
use crate::styles::theme::{Theme, ThemeMode};
use crate::ui::{Hot, Layout, SidebarEntry, UiState, TAB_BAR_HEIGHT};
use super::*;

impl MainWindow {
    /// The primary button of the `ContentDialog` (`DefaultButton="Primary"`).
    /// `GetFor_CreateItemDialog`: "Create a new {folder|file}", name
    /// field (pre-filled with "New folder" selected, like the original's
    /// TextBox — #17845).
    pub(crate) fn open_create_item_dialog(&mut self, folder: bool) {
        let tr = kubuno_drive_desktop_localization::tr;
        let kind = tr(if folder { "Folder" } else { "File" }).to_lowercase();
        let title_fmt = tr("CreateNewItemTitle").replace("{0}", &kind);
        let mut dialog = crate::dialogs::DialogState::simple(
            title_fmt,
            tr("EnterAnItemName").to_string(),
            tr("Create").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::CreateItem(folder),
        );
        dialog.field_label = Some(tr("Name").to_string());
        let text = if folder { tr("NewFolder").to_string() } else { String::new() };
        self.state.dialog = Some(dialog);
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_DIALOG,
            caret: text.len(),
            anchor: 0,
            text,
        });
        self.invalidate();
    }

    /// `CreateShortcutDialog.xaml`: the target path for a new shortcut.
    pub(crate) fn open_create_shortcut_dialog(&mut self) {
        let tr = kubuno_drive_desktop_localization::tr;
        let mut dialog = crate::dialogs::DialogState::simple(
            tr("NewShortcutDialogTitle").to_string(),
            tr("NewShortcutDialogDescription").to_string(),
            tr("Create").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::CreateShortcutTo,
        );
        dialog.field_label = Some(tr("ExtractToPath").to_string()); // "Path"
        self.state.dialog = Some(dialog);
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_DIALOG,
            caret: 0,
            anchor: 0,
            text: String::new(),
        });
        self.invalidate();
    }

    pub(crate) fn dialog_primary(&mut self) {
        use crate::dialogs::DialogAction;
        let Some(dialog) = self.state.dialog.take() else { return };
        match &dialog.action {
            DialogAction::DeleteItems(paths) => {
                // The "Permanently delete" checkbox bypasses the
                // recycle bin; items ALREADY in the Recycle Bin go through
                // `IFileOperation` (the `$I` must follow the `$R`) and are
                // always permanent (`FilesystemHelpers.DeleteItemsAsync`).
                let ok = if paths.iter().any(|p| crate::services::storage::storage_trash_bin_service::is_under_trash_bin(p))
                {
                    crate::services::storage::storage_trash_bin_service::delete(paths)
                } else {
                    !paths.is_empty()
                        && crate::utils::storage::delete_items(paths, dialog.checkbox)
                };
                if ok {
                    self.state.active_mut().refresh();
                }
            }
            // The three Recycle Bin confirmations (`StorageTrashBinService`).
            DialogAction::EmptyRecycleBin => {
                if crate::services::storage::storage_trash_bin_service::empty() {
                    self.state.active_mut().refresh();
                }
            }
            DialogAction::RestoreAllTrashes => {
                if crate::services::storage::storage_trash_bin_service::restore_all() {
                    self.state.active_mut().refresh();
                }
            }
            DialogAction::RestoreTrashItems(paths) => {
                if crate::services::storage::storage_trash_bin_service::restore(paths) {
                    self.state.active_mut().refresh();
                }
            }
            // `CreateArchiveDialog`: the name comes from the field, the format from the choice.
            DialogAction::CompressInto(sources) => {
                let name = self
                    .state
                    .edit
                    .take()
                    .filter(|e| e.entry == crate::ui::EDIT_DIALOG)
                    .map(|e| e.text)
                    .unwrap_or_default();
                let ext = if dialog.choice == 1 { "7z" } else { "zip" };
                if let Some(parent) = sources
                    .first()
                    .and_then(|s| std::path::Path::new(s).parent())
                {
                    let archive = parent.join(format!("{name}.{ext}"));
                    if !name.is_empty() {
                        crate::utils::storage::compress(sources, &archive.to_string_lossy());
                        self.state.active_mut().refresh();
                    }
                }
            }
            // The AddItemDialog has no primary button: its items chain
            // directly (Hot::DialogItem).
            DialogAction::AddItem => {}
            // `CreateFileFromDialogResultTypeAsync`: create the named item
            // in the current folder (collision → suffix, GenerateUniqueName).
            DialogAction::CreateItem(folder) => {
                let name = self
                    .state
                    .edit
                    .take()
                    .filter(|e| e.entry == crate::ui::EDIT_DIALOG)
                    .map(|e| e.text)
                    .unwrap_or_default();
                let name = if name.trim().is_empty() {
                    if *folder {
                        kubuno_drive_desktop_localization::tr("NewFolder").to_string()
                    } else {
                        String::new()
                    }
                } else {
                    name
                };
                if let (false, Location::Dir(dir)) =
                    (name.is_empty(), &self.state.active().location)
                {
                    let path = crate::utils::storage::unique_path(&dir.join(&name));
                    let created = if *folder {
                        std::fs::create_dir(&path).is_ok()
                    } else {
                        std::fs::File::create_new(&path).is_ok()
                    };
                    if created {
                        self.state.active_mut().refresh();
                    }
                }
            }
            // `FileTagsHelper.RemoveTagsAsync`: the confirmation dialog's Yes
            // clears the tags of the entire selection.
            DialogAction::RemoveTags(paths) => {
                for path in paths {
                    crate::utils::file_tags::write_file_tags(path, &[]);
                }
                self.state.active_mut().refresh();
            }
            // `CreateShortcutDialogViewModel.CreateShortcutAsync`: `.lnk` if
            // the target exists, `.url` otherwise; auto name "target - Shortcut".
            DialogAction::CreateShortcutTo => {
                let target = self
                    .state
                    .edit
                    .take()
                    .filter(|e| e.entry == crate::ui::EDIT_DIALOG)
                    .map(|e| e.text)
                    .unwrap_or_default();
                let target = target.trim().to_string();
                if let (false, Location::Dir(dir)) =
                    (target.is_empty(), &self.state.active().location)
                {
                    let exists = std::path::Path::new(&target).exists();
                    let leaf = std::path::Path::new(target.trim_end_matches(['\\', '/']))
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| target.clone());
                    let ext = if exists { "lnk" } else { "url" };
                    let name =
                        format!("{leaf} - {}.{ext}", kubuno_drive_desktop_localization::tr("Shortcut"));
                    let path = crate::utils::storage::unique_path(&dir.join(name));
                    let created = if exists {
                        crate::utils::storage::create_shortcut(&target, &path.to_string_lossy())
                    } else {
                        std::fs::write(&path, format!("[InternetShortcut]\r\nURL={target}\r\n"))
                            .is_ok()
                    };
                    if created {
                        self.state.active_mut().refresh();
                    }
                }
            }
            // `DecompressArchiveDialog`: the destination comes from the field; the
            // checkbox opens the folder once extraction is done.
            DialogAction::DecompressTo(archive) => {
                let dest = self
                    .state
                    .edit
                    .take()
                    .filter(|e| e.entry == crate::ui::EDIT_DIALOG)
                    .map(|e| e.text)
                    .unwrap_or_default();
                if !dest.trim().is_empty()
                    && crate::utils::storage::decompress(archive, &dest)
                {
                    if dialog.checkbox {
                        self.navigate_active(Location::Dir(dest.into()));
                    } else {
                        self.state.active_mut().refresh();
                    }
                }
            }
            // `CreateNewBranchAsync`: creates the named branch at HEAD and
            // switches to it (git operation off the UI thread).
            DialogAction::GitCreateBranch => {
                let name = self
                    .state
                    .edit
                    .take()
                    .filter(|e| e.entry == crate::ui::EDIT_DIALOG)
                    .map(|e| e.text.trim().to_string())
                    .unwrap_or_default();
                if let (false, Some(dir)) = (name.is_empty(), self.git_dir()) {
                    self.run_git_op(move || crate::utils::git::create_branch(&dir, &name));
                }
            }
        }
        self.invalidate();
    }

}
