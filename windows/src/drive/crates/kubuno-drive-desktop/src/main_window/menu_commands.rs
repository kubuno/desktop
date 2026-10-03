#![allow(unused_imports)]
//! Menu command dispatch and shell overflow (mirror of `Actions/*` / `ShellContextMenu`) — `impl MainWindow` block, see `main_window/mod.rs`.
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
    /// Screen pixels → client DIP (the inverse of `client_to_screen_px`).
    pub(crate) fn request_shell_overflow(&mut self, path: String) {
        let worker = self
            .shell_worker
            .get_or_insert_with(|| crate::utils::shell::ShellWorker::new(self.hwnd, WM_APP_SHELL_MENU));
        worker.open(path);
    }

    /// Swaps the "Loading…" row for the shell entries once the worker
    /// reports back (and refreshes the view if a shell verb just ran).
    pub(crate) fn load_shell_overflow(&mut self) {
        use crate::ui::{FlyoutItem, MenuCommand};
        let Some(worker) = &self.shell_worker else {
            return;
        };
        if std::mem::take(&mut *worker.invoked.lock().unwrap()) {
            self.state.active_mut().refresh();
            self.model = HomeModel::load();
            self.invalidate();
            return;
        }
        let Some(entries) = worker.result.lock().unwrap().clone() else {
            return;
        };
        let children: Vec<FlyoutItem> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if e.separator {
                    FlyoutItem::separator()
                } else {
                    FlyoutItem::new(e.label.clone(), e.enabled)
                        .with_bitmap(e.bitmap.clone())
                        .with_command(MenuCommand::ShellCommand(i))
                }
            })
            .collect();
        if children.is_empty() {
            return;
        }

        let width = self.flyout_width(&children);
        if let Some(overflow) = self
            .state
            .flyout
            .as_mut()
            .and_then(|f| f.items.iter_mut().find(|it| it.command == MenuCommand::ShowMoreOptions))
        {
            overflow.children = children;
            overflow.children_width = width;
            overflow.has_submenu = true;
        }
        self.invalidate();
    }

    /// The current folder's path, if it's under version control (target of
    /// git operations). `None` outside a folder.
    /// Runs one `ICommandManager` command on `path` (the flyout's target).
    pub(crate) fn run_menu_command(&mut self, command: crate::ui::MenuCommand, path: Option<String>) {
        use crate::ui::MenuCommand as C;
        // The menu's target is passed to the action via `parameter`, like
        // `IRichCommand.ExecuteAsync(object?)` in the original.
        fn act(action: &dyn crate::actions::Action, w: &mut MainWindow, path: &str) {
            action.execute(w, if path.is_empty() { None } else { Some(path) });
        }
        let path = path.unwrap_or_default();
        match command {
            C::None => {}
            // Checks/unchecks a tag on the pane's item (NTFS
            // `:files` stream, port of WriteFileTag).
            C::ToggleFileTag(i) => {
                if let Some(tag) = crate::services::settings::get().file_tags.get(i) {
                    if !path.is_empty() {
                        crate::utils::file_tags::toggle_file_tag(&path, &tag.uid);
                    }
                }
                self.invalidate();
            }
            // Delegated to the registry (`actions/display/`).
            C::LayoutDetails => {
                use crate::actions::Action;
                crate::actions::display::layout_action::LayoutDetails.execute(self, None)
            }
            C::LayoutList => {
                use crate::actions::Action;
                crate::actions::display::layout_action::LayoutList.execute(self, None)
            }
            C::LayoutCards => {
                use crate::actions::Action;
                crate::actions::display::layout_action::LayoutCards.execute(self, None)
            }
            C::LayoutGrid => {
                use crate::actions::Action;
                crate::actions::display::layout_action::LayoutGrid.execute(self, None)
            }
            C::LayoutColumns => {
                use crate::actions::Action;
                crate::actions::display::layout_action::LayoutColumns.execute(self, None)
            }
            C::GroupByNone
            | C::GroupByName
            | C::GroupByDateModified
            | C::GroupByType
            | C::GroupBySize
            | C::GroupAscending
            | C::GroupDescending => {
                use crate::actions::display::group_action as ga;
                use crate::actions::Action;
                let action: &dyn Action = match command {
                    C::GroupByNone => &ga::GroupByNone,
                    C::GroupByName => &ga::GroupByName,
                    C::GroupByDateModified => &ga::GroupByDateModified,
                    C::GroupByType => &ga::GroupByType,
                    C::GroupBySize => &ga::GroupBySize,
                    C::GroupAscending => &ga::GroupAscending,
                    _ => &ga::GroupDescending,
                };
                action.execute(self, None);
            }
            C::SortByName
            | C::SortByDateModified
            | C::SortByType
            | C::SortBySize
            | C::SortByOriginalPath
            | C::SortByDateDeleted => {
                use crate::actions::display::sort_action as sa;
                use crate::actions::Action;
                match command {
                    C::SortByName => sa::SortByName.execute(self, None),
                    C::SortByDateModified => sa::SortByDateModified.execute(self, None),
                    C::SortByType => sa::SortByType.execute(self, None),
                    // Recycle Bin only (`SortByOriginalFolderAction` /
                    // `SortByDateDeletedAction` from SortAction.cs).
                    C::SortByOriginalPath => sa::SortByOriginalFolder.execute(self, None),
                    C::SortByDateDeleted => sa::SortByDateDeleted.execute(self, None),
                    _ => sa::SortBySize.execute(self, None),
                }
            }
            C::SortAscending | C::SortDescending => {
                use crate::actions::display::sort_action as sa;
                use crate::actions::Action;
                if command == C::SortAscending {
                    sa::SortAscending.execute(self, None)
                } else {
                    sa::SortDescending.execute(self, None)
                }
            }
            C::RefreshItems => {
                self.state.active_mut().refresh();
                self.model = HomeModel::load();
            }
            C::RestoreRecycleBin => {
                act(&crate::actions::file_system::RestoreRecycleBin, self, &path)
            }
            C::EmptyRecycleBin => act(&crate::actions::file_system::EmptyRecycleBin, self, &path),
            C::RestoreAllRecycleBin => {
                act(&crate::actions::file_system::RestoreAllRecycleBin, self, &path)
            }
            C::CreateFolder => self.create_item(&path, true),
            C::CreateFile => self.create_item(&path, false),
            C::CreateShellNew(i) => self.create_from_shell_new(&path, i),
            C::CreateShortcutFromDialog => {
                act(&crate::actions::file_system::CreateShortcutFromDialog, self, &path)
            }
            // Most commands delegate to the `actions/` mirror.
            C::OpenTerminal => act(&crate::actions::open::OpenTerminal, self, &path),
            C::OpenItem => act(&crate::actions::file_system::OpenItem, self, &path),
            C::OpenItemWithApplicationPicker => {
                act(&crate::actions::open::OpenItemWithApplicationPicker, self, &path)
            }
            C::OpenInNewTab => act(&crate::actions::navigation::OpenInNewTab, self, &path),
            C::OpenInNewWindow => act(&crate::actions::navigation::OpenInNewWindow, self, &path),
            C::CutItem => {
                crate::utils::storage::clipboard_set_files(self.hwnd, std::slice::from_ref(&path), true);
            }
            C::CopyItem => {
                crate::utils::storage::clipboard_set_files(self.hwnd, std::slice::from_ref(&path), false);
            }
            C::PasteItem => self.paste_clipboard(),
            C::CopyItemPath => act(&crate::actions::file_system::CopyItemPath, self, &path),
            C::Rename => self.begin_rename(),
            C::ShareItem => act(&crate::actions::content::share::ShareItem, self, &path),
            C::DeleteItem => {
                if crate::utils::storage::delete_items(std::slice::from_ref(&path), false) {
                    self.state.active_mut().refresh();
                }
            }
            C::OpenProperties => act(&crate::actions::open::OpenProperties, self, &path),
            C::PinFolderToSidebar => {
                act(&crate::actions::sidebar::PinFolderToSidebar, self, &path)
            }
            C::UnpinFolderFromSidebar => {
                act(&crate::actions::sidebar::UnpinFolderFromSidebar, self, &path)
            }
            // `ItemOverflow` only opens its submenu; it runs nothing itself.
            C::ShowMoreOptions => {}
            C::ShellCommand(i) => {
                // The verb runs on the worker thread (the one holding
                // the IContextMenu); the refresh follows its message.
                if let Some(worker) = &self.shell_worker {
                    worker.invoke(i);
                }
            }
            C::OpenFileLocation => {
                if let Some(target) = crate::utils::storage::resolve_shortcut(&path) {
                    if let Some(parent) = std::path::Path::new(&target).parent() {
                        self.navigate_active(Location::Dir(parent.to_path_buf()));
                    }
                }
            }
            C::OpenInNewPaneVertical | C::OpenInNewPaneHorizontal => {
                self.state.group_mut().split(command == C::OpenInNewPaneVertical);
                self.navigate_active(Location::Dir(path.clone().into()));
            }
            C::CloseActivePane => {
                self.state.group_mut().close_active_pane();
                self.invalidate();
            }
            C::RunAsAdmin => act(&crate::actions::content::run::RunAsAdmin, self, &path),
            C::RunAsAnotherUser => {
                act(&crate::actions::content::run::RunAsAnotherUser, self, &path)
            }
            C::PasteItemAsShortcut => {
                act(&crate::actions::file_system::PasteItemAsShortcut, self, &path)
            }
            C::CreateShortcut => act(&crate::actions::file_system::CreateShortcut, self, &path),
            C::CreateFolderWithSelection => {
                act(&crate::actions::file_system::CreateFolderWithSelection, self, &path)
            }
            C::CompressIntoArchive => {
                act(&crate::actions::content::archives::CompressIntoArchive, self, &path)
            }
            C::CompressIntoZip => {
                act(&crate::actions::content::archives::CompressIntoZip, self, &path)
            }
            C::CompressIntoSevenZip => {
                act(&crate::actions::content::archives::CompressIntoSevenZip, self, &path)
            }
            C::DecompressArchive => {
                act(&crate::actions::content::archives::DecompressArchive, self, &path)
            }
            C::DecompressArchiveHere => {
                act(&crate::actions::content::archives::DecompressArchiveHere, self, &path)
            }
            C::DecompressArchiveToChildFolder => act(
                &crate::actions::content::archives::DecompressArchiveToChildFolder,
                self,
                &path,
            ),
            C::FlattenFolder => act(&crate::actions::file_system::FlattenFolder, self, &path),
            C::EditInNotepad => act(&crate::actions::open::EditInNotepad, self, &path),
            // Images: rotate / set as background. These actions operate on the
            // current SELECTION; the clicked `path` is only passed as a fallback.
            C::RotateLeft => {
                use crate::actions::Action;
                crate::actions::content::image_manipulation::RotateLeft.execute(self, None)
            }
            C::RotateRight => {
                use crate::actions::Action;
                crate::actions::content::image_manipulation::RotateRight.execute(self, None)
            }
            C::SetAsWallpaperBackground => {
                act(&crate::actions::content::background::SetAsWallpaperBackground, self, &path)
            }
            C::SetAsLockscreenBackground => {
                act(&crate::actions::content::background::SetAsLockscreenBackground, self, &path)
            }
            C::SetAsSlideshowBackground => {
                use crate::actions::Action;
                crate::actions::content::background::SetAsSlideshowBackground.execute(self, None)
            }
            C::SetAsAppBackground => {
                act(&crate::actions::content::background::SetAsAppBackground, self, &path)
            }
            // Drive operations (formatting opens the Windows dialog; nothing
            // is erased without the user clicking inside it).
            C::FormatDrive => act(&crate::actions::file_system::FormatDrive, self, &path),
            C::EjectDrive => crate::utils::drive_helpers::eject_drive(self.hwnd, &path),
            C::OpenStorageSense => {
                use crate::actions::Action;
                crate::actions::open::OpenStorageSense.execute(self, None)
            }
            C::SendTo(i) => {
                if let Some((_, link)) = crate::utils::storage::sendto_entries().into_iter().nth(i) {
                    // Explorer passes the item as the shortcut's argument.
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "", &link, &path])
                        .spawn();
                }
            }
        }
        self.invalidate();
    }
}
