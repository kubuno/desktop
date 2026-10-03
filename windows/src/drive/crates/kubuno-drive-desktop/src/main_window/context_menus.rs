#![allow(unused_imports)]
//! Sous-module de `MainWindow` — voir `main_window/mod.rs`.
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
    pub(crate) fn open_flyout(
        &mut self,
        kind: crate::ui::FlyoutKind,
        items: Vec<crate::ui::FlyoutItem>,
        path: Option<String>,
        x_px: f32,
        y_px: f32,
    ) {
        self.open_flyout_with_primary(kind, items, Vec::new(), path, x_px, y_px);
    }

    /// Overrides the hit-test with the pane's Properties button, whose
    /// position (within the scrolling FLOW) is recorded by the drawing pass.
    pub(crate) fn open_edit_tags_flyout(&mut self, x_px: f32, y_px: f32) {
        let Some(path) = self.info_pane_path() else { return };
        let assigned = crate::utils::file_tags::read_file_tags(&path);
        let items: Vec<crate::ui::FlyoutItem> = crate::services::settings::get()
            .file_tags
            .iter()
            .enumerate()
            .map(|(i, tag)| {
                crate::ui::FlyoutItem::new(tag.name.clone(), true)
                    .checked(assigned.contains(&tag.uid))
                    .with_command(crate::ui::MenuCommand::ToggleFileTag(i))
            })
            .collect();
        if !items.is_empty() {
            // ItemContext: the ONLY kind (along with EmptySpace/SidebarContext)
            // that `on_flyout_click` dispatches BY COMMAND with the carried path.
            self.open_flyout(crate::ui::FlyoutKind::ItemContext, items, Some(path), x_px, y_px);
        }
    }

    pub(crate) fn open_flyout_with_primary(
        &mut self,
        kind: crate::ui::FlyoutKind,
        items: Vec<crate::ui::FlyoutItem>,
        primary: Vec<crate::ui::FlyoutItem>,
        path: Option<String>,
        x_px: f32,
        y_px: f32,
    ) {
        let mut items = items;
        for item in &mut items {
            if !item.children.is_empty() {
                item.children_width = self.flyout_width(&item.children);
            }
        }
        // The panel must fit the widest menu row and, at minimum, the command
        // bar's buttons (40 DIP each, with the bar's 4 DIP side padding).
        let bar = if primary.is_empty() { 0.0 } else { 8.0 + primary.len() as f32 * 40.0 };
        let fw = self.flyout_width(&items);
        let width = fw.max(bar);
        // Anchor in client DIP; the popup window itself is clamped to the
        // monitor work area in `sync_flyout`, so it may extend past the app.
        self.state.flyout = Some(crate::ui::Flyout {
            kind,
            x: self.to_dip(x_px),
            y: self.to_dip(y_px),
            width,
            items,
            primary,
            submenu: None,
            opened: std::time::Instant::now(),
            submenu_opened: None,
            sub_pos: None,
            subsubmenu: None,
            subsubmenu_opened: None,
            subsub_pos: None,
            hot_sub2: None,
            layout: None,
            picker: None,
            hot: None,
            hot_primary: None,
            path,
        });
        self.invalidate();
    }


    /// Port of `ContentPageContextFlyoutFactory.GetBaseItemMenuItems` with a
    /// selection: the commands Files runs itself, then `ShowMoreOptions`, which
    /// is where the shell's own extensions live.
    pub(crate) fn show_item_context_menu(&mut self, path: String, x_px: f32, y_px: f32) {
        use crate::ui::{FlyoutItem, MenuCommand};
        let tr = kubuno_drive_desktop_localization::tr;

        // Recycle bin (`Check(ShowInRecycleBin)`): Restore + the primary
        // Delete / Properties — everything else is filtered by the factory.
        if self.state.active().location == Location::RecycleBin {
            let primary = vec![
                FlyoutItem::new(tr("Delete").to_string(), true)
                    .with_icon("Delete")
                    .with_command(MenuCommand::DeleteItem),
                FlyoutItem::new(tr("OpenProperties").to_string(), true)
                    .with_icon("Properties")
                    .with_command(MenuCommand::OpenProperties),
            ];
            let items = vec![FlyoutItem::new(tr("Restore").to_string(), true)
                .with_icon("RestoreDeleted")
                .with_command(MenuCommand::RestoreRecycleBin)];
            self.open_flyout_with_primary(
                crate::ui::FlyoutKind::ItemContext,
                items,
                primary,
                Some(path),
                x_px,
                y_px,
            );
            return;
        }
        let is_dir = std::path::Path::new(&path).is_dir();
        let pinned = self.model.quick_access.iter().any(|q| q.path == path);

        // `GetBaseItemMenuItems` evaluates its predicates over the WHOLE
        // selection (LINQ `All`/`Any`), not just the clicked item.
        // `selected_paths()` is in display order; falls back to `path` for
        // Home cards (QuickCard/DriveCard) where the list selection is empty.
        let sel: Vec<String> = {
            let s = self.state.active().selected_paths();
            if s.is_empty() { vec![path.clone()] } else { s }
        };
        let count = sel.len();
        // `areAllItemsFolders = selectedItems.All(Folder)`.
        let all_folders = sel.iter().all(|p| std::path::Path::new(p).is_dir());
        // `selectedItems.FirstOrDefault()`: the FIRST of the selection
        // (display order), NOT the clicked item — don't "fix it up" to `path`.
        let first = sel.first().cloned().unwrap_or_else(|| path.clone());
        let first_ext = std::path::Path::new(&first)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let first_is_shortcut = first_ext == "lnk" || first_ext == "url";
        let first_is_executable = matches!(first_ext.as_str(), "exe" | "bat" | "cmd" | "ahk");

        // `CommandBarFlyout.PrimaryCommands`: the icon-only band the factory
        // fills with its `IsPrimary = true` entries, in this exact order. A
        // builder hides an entry when `IsVisible` is unset AND the command is
        // not executable — hence Share, which `ShareItemHelpers.IsItemShareable`
        // rejects for plain folders.
        let mut primary = vec![
            FlyoutItem::new(tr("Cut").to_string(), true)
                .with_icon("Cut")
                .with_command(MenuCommand::CutItem),
            FlyoutItem::new(tr("Copy").to_string(), true)
                .with_icon("Copy")
                .with_command(MenuCommand::CopyItem),
            // `IsVisible = true`: always shown, greyed out when the clipboard
            // holds nothing to paste.
            FlyoutItem::new(tr("Paste").to_string(), crate::utils::storage::clipboard_has_files())
                .with_icon("Paste")
                .with_command(MenuCommand::PasteItem),
            FlyoutItem::new(tr("Rename").to_string(), true)
                .with_icon("Rename")
                .with_command(MenuCommand::Rename),
        ];
        // `ShareItemHelpers.IsItemShareable`: Share hidden only when the WHOLE
        // selection is folders (approximation of the C# executable predicate).
        if !all_folders {
            primary.push(
                FlyoutItem::new(tr("Share").to_string(), true)
                    .with_icon("Share")
                    .with_command(MenuCommand::ShareItem),
            );
        }
        primary.push(
            FlyoutItem::new(tr("Delete").to_string(), true)
                .with_icon("Delete")
                .with_command(MenuCommand::DeleteItem),
        );
        primary.push(
            FlyoutItem::new(tr("OpenProperties").to_string(), true)
                .with_icon("Properties")
                .with_command(MenuCommand::OpenProperties),
        );

        // `GetBaseItemMenuItems` with `itemsSelected == true`, in factory order.
        // The flags below are the C# locals of the same name.
        let ext = std::path::Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let is_shortcut = ext == "lnk" || ext == "url";
        let is_executable = matches!(ext.as_str(), "exe" | "bat" | "cmd" | "ahk");
        let is_batch = matches!(ext.as_str(), "bat" | "cmd" | "ahk");
        // `showOpenItemWith`: a single plain file — never a folder, a shortcut
        // or an executable.
        let show_open_with = count == 1 && !is_dir && !is_shortcut && !is_executable;
        // `canDecompress = Any() && All(IsArchive)`; `canCompress = !canDecompress
        // || Count > 1` — the two coexist in multi-selection.
        let can_decompress =
            !sel.is_empty() && sel.iter().all(|p| crate::utils::storage::is_archive(p));
        let can_compress = !can_decompress || count > 1;
        let can_paste = crate::utils::storage::clipboard_has_files();

        let mut items = vec![
            FlyoutItem::new(tr("Open").to_string(), true)
                .with_icon("OpenFile")
                .with_command(MenuCommand::OpenItem),
        ];
        if show_open_with {
            items.push(
                FlyoutItem::new(tr("OpenWith").to_string(), true)
                    .with_icon("OpenWith")
                    .with_command(MenuCommand::OpenItemWithApplicationPicker),
            );
        }
        if is_shortcut {
            items.push(
                FlyoutItem::new(tr("OpenFileLocation").to_string(), true)
                    .with_glyph("\u{E8DA}")
                    .with_command(MenuCommand::OpenFileLocation),
            );
        }
        if all_folders {
            items.push(
                FlyoutItem::new(tr("OpenInNewTab").to_string(), true)
                    .with_icon("OpenInTab")
                    .with_command(MenuCommand::OpenInNewTab),
            );
            items.push(
                FlyoutItem::new(tr("OpenInNewWindow").to_string(), true)
                    .with_icon("OpenInWindow")
                    // Original HotKey: Menu+Ctrl+Enter, displayed Alt first.
                    .with_accel([key_name(0x12), key_name(0x11), key_name(0x0D)].join("+"))
                    .with_command(MenuCommand::OpenInNewWindow),
            );
            items.push(
                FlyoutItem::new(tr("OpenInNewPane").to_string(), true)
                    .with_icon("OpenInPaneVertical")
                    .with_children(vec![
                        FlyoutItem::new(tr("SplitPaneVertically").to_string(), true)
                            .with_icon("OpenInPaneVertical")
                            .with_command(MenuCommand::OpenInNewPaneVertical),
                        FlyoutItem::new(tr("SplitPaneHorizontally").to_string(), true)
                            .with_icon("OpenInPaneHorizontal")
                            .with_command(MenuCommand::OpenInNewPaneHorizontal),
                    ]),
            );
        }
        if first_is_executable {
            items.push(
                FlyoutItem::new(tr("RunAsAdministrator").to_string(), true)
                    .with_glyph("\u{E7EF}")
                    .with_command(MenuCommand::RunAsAdmin),
            );
            items.push(
                FlyoutItem::new(tr("BaseLayoutContextFlyoutRunAsAnotherUser.Text").to_string(), true)
                    .with_glyph("\u{E7EE}")
                    .with_command(MenuCommand::RunAsAnotherUser),
            );
        }
        items.push(FlyoutItem::separator());
        // Like the original Builder (IsVisible = IsExecutable): HIDDEN when the
        // clipboard is empty, not greyed out.
        if can_paste {
            items.push(
                FlyoutItem::new(tr("PasteShortcut").to_string(), true)
                    .with_icon("Paste")
                    .with_command(MenuCommand::PasteItemAsShortcut),
            );
        }
        items.push(
            FlyoutItem::new(tr("CopyItemPath").to_string(), true)
                .with_icon("CopyAsPath")
                .with_accel(hotkey_text(&["Control", "Shift"], "C"))
                .with_command(MenuCommand::CopyItemPath),
        );
        items.push(
            FlyoutItem::new(tr("CreateFolderWithSelection").to_string(), true)
                .with_icon("NewFolder")
                .with_command(MenuCommand::CreateFolderWithSelection),
        );
        if !first_is_shortcut {
            items.push(
                FlyoutItem::new(tr("CreateShortcut").to_string(), true)
                    .with_icon("ShortcutUrl")
                    .with_command(MenuCommand::CreateShortcut),
            );
        }
        if all_folders {
            items.push(if pinned {
                FlyoutItem::new(tr("UnpinFolderFromSidebar").to_string(), true)
                    .with_icon("FavoritePin")
                    .with_command(MenuCommand::UnpinFolderFromSidebar)
            } else {
                FlyoutItem::new(tr("PinFolderToSidebar").to_string(), true)
                    .with_icon("FavoritePin")
                    .with_command(MenuCommand::PinFolderToSidebar)
            });
        }
        // Compress / Extract: `canCompress = !canDecompress || Count > 1`,
        // so the two can COEXIST in multi-selection → two independent `if`s
        // (Extract first, then Compress). `newArchiveName` = the item's name
        // if alone, otherwise the PARENT FOLDER's name (`GetDirectoryName`).
        let archive_stem = if count == 1 {
            crate::utils::storage::archive_name(&first)
        } else {
            std::path::Path::new(&first)
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        if can_decompress {
            let stem = &archive_stem;
            items.push(
                FlyoutItem::new(tr("Extract").to_string(), true).with_icon("Zip").with_children(
                    vec![
                        FlyoutItem::new(tr("ExtractFiles").to_string(), true)
                            .with_icon("Zip")
                            .with_command(MenuCommand::DecompressArchive),
                        FlyoutItem::new(tr("ExtractHere").to_string(), true)
                            .with_icon("Zip")
                            .with_command(MenuCommand::DecompressArchiveHere),
                        FlyoutItem::new(
                            tr("BaseLayoutItemContextFlyoutExtractToChildFolder")
                                .replace("{0}", stem),
                            true,
                        )
                        .with_icon("Zip")
                        .with_command(MenuCommand::DecompressArchiveToChildFolder),
                    ],
                ),
            );
        }
        if can_compress {
            let stem = &archive_stem;
            let named = |name: &str| tr("CreateNamedArchive").replace("{0}", name);
            items.push(
                FlyoutItem::new(tr("Compress").to_string(), true).with_icon("Zip").with_children(
                    vec![
                        FlyoutItem::new(tr("CreateArchive").to_string(), true)
                            .with_icon("Zip")
                            .with_command(MenuCommand::CompressIntoArchive),
                        FlyoutItem::new(named(&format!("{stem}.zip")), true)
                            .with_icon("Zip")
                            .with_command(MenuCommand::CompressIntoZip),
                        FlyoutItem::new(named(&format!("{stem}.7z")), true)
                            .with_icon("Zip")
                            .with_command(MenuCommand::CompressIntoSevenZip),
                    ],
                ),
            );
        }
        // `FlattenFolderAction.IsExecutable` requires ShowFlattenOptions
        // (general setting, off by default) besides the single selected folder.
        if all_folders && crate::services::settings::get().show_flatten_options {
            items.push(
                FlyoutItem::new(tr("FlattenFolder").to_string(), true)
                    .with_icon("Folder")
                    .with_command(MenuCommand::FlattenFolder),
            );
        }
        // Images (`SetAsSubMenu` + Rotate from the factory): « Définir comme fond »
        // (Desktop / Lockscreen / Slideshow / App) and Rotate left/right.
        if !all_folders
            && sel
                .iter()
                .all(|p| crate::actions::content::image_manipulation::is_wallpaper_compatible(p))
        {
            let multi = self.state.active().selected.len() > 1;
            items.push(
                FlyoutItem::new(tr("BaseLayoutItemContextFlyoutSetAs.Text").to_string(), true)
                    .with_glyph("\u{E91B}")
                    .with_children(vec![
                        FlyoutItem::new(tr("Desktop").to_string(), true)
                            .with_command(MenuCommand::SetAsWallpaperBackground),
                        FlyoutItem::new(tr("Lockscreen").to_string(), true)
                            .with_command(MenuCommand::SetAsLockscreenBackground),
                        FlyoutItem::new(tr("Slideshow").to_string(), multi)
                            .with_command(MenuCommand::SetAsSlideshowBackground),
                        FlyoutItem::new(tr("SetAsAppBackground").to_string(), true)
                            .with_command(MenuCommand::SetAsAppBackground),
                    ]),
            );
            items.push(
                FlyoutItem::new(tr("RotateLeft").to_string(), true)
                    .with_glyph("\u{E7AD}")
                    .with_command(MenuCommand::RotateLeft),
            );
            items.push(
                FlyoutItem::new(tr("RotateRight").to_string(), true)
                    .with_glyph("\u{E7AD}")
                    .with_command(MenuCommand::RotateRight),
            );
        }
        // Drive root: Eject, Storage Sense, Format
        // (`FormatDriveAction` + the factory's drive menu). « C:\ » (system)
        // is not formattable.
        if is_drive_root(&path) {
            items.push(FlyoutItem::separator());
            items.push(
                FlyoutItem::new(tr("Eject").to_string(), true)
                    .with_glyph("\u{E8DA}")
                    .with_command(MenuCommand::EjectDrive),
            );
            items.push(
                FlyoutItem::new(tr("OpenStorageSense").to_string(), true)
                    .with_glyph("\u{E9D9}")
                    .with_command(MenuCommand::OpenStorageSense),
            );
            if !path.trim_end_matches('\\').eq_ignore_ascii_case("C:") {
                items.push(
                    FlyoutItem::new(tr("FormatDriveText").to_string(), true)
                        .with_glyph("\u{E954}")
                        .with_command(MenuCommand::FormatDrive),
                );
            }
        }
        // « Envoyer vers »: no icon in the original, submenu filled by the
        // shell — here by the user's SendTo folder.
        let sendto: Vec<FlyoutItem> = crate::utils::storage::sendto_entries()
            .into_iter()
            .enumerate()
            .map(|(i, (label, _))| {
                FlyoutItem::new(label, true).with_command(MenuCommand::SendTo(i))
            })
            .collect();
        if !sendto.is_empty() {
            items.push(FlyoutItem::new(tr("SendTo").to_string(), true).with_children(sendto));
        }
        if is_batch {
            items.push(
                FlyoutItem::new(tr("EditInNotepad").to_string(), true)
                    .with_glyph("\u{E70F}")
                    .with_command(MenuCommand::EditInNotepad),
            );
        }
        if all_folders {
            items.push(FlyoutItem::separator());
            items.push(
                FlyoutItem::new(tr("OpenTerminal").to_string(), true)
                    .with_glyph("\u{E756}")
                    // Ctrl+Oem3 (VK 0xC0): « ù » on a FR keyboard, layout-dependent.
                    .with_accel(hotkey_text_vk(0xC0, true, false, false))
                    .with_command(MenuCommand::OpenTerminal),
            );
        }
        // `OverflowSeparator` then the `ItemOverflow` entry: it opens on a
        // disabled « Chargement … », which `WM_APP_SHELL_MENU` replaces with
        // the shell's items (port of `AddShellMenuItemsAsync`).
        items.push(FlyoutItem::separator());
        items.push(
            FlyoutItem::new(tr("ShowMoreOptions").to_string(), true)
                .with_glyph("\u{E712}")
                .with_children(vec![FlyoutItem::new(tr("Loading").to_string(), false)])
                .with_command(MenuCommand::ShowMoreOptions),
        );
        self.request_shell_overflow(path.clone());
        self.open_flyout_with_primary(
            crate::ui::FlyoutKind::ItemContext,
            items,
            primary,
            Some(path),
            x_px,
            y_px,
        );
    }

    /// The same factory with `itemsSelected == false`: Disposition / Trier par /
    /// Grouper par / Actualiser / Nouveau / Terminal.
    pub(crate) fn show_empty_space_menu(&mut self, x_px: f32, y_px: f32) {
        use crate::view_models::shell_view_model::{GroupOption, SortColumn, ViewMode};
        use crate::ui::{FlyoutItem, MenuCommand};
        let tr = kubuno_drive_desktop_localization::tr;
        let tab = self.state.active();
        let (mode, sort, ascending) = (tab.view_mode, tab.sort_column, tab.sort_ascending);
        let (group, group_asc) = (tab.group_option, tab.group_ascending);
        let dir = matches!(tab.location, Location::Dir(_));

        // `Layout`: the 5 view modes (`LayoutSubMenu` from the factory). The
        // 28px icons (`LayoutList28`…) are drawn at the menu's size.
        let layout_menu = FlyoutItem::new(tr("Layout").to_string(), true)
            .with_glyph("\u{E8A9}")
            .with_children(
                ViewMode::ALL
                    .iter()
                    .map(|&m| {
                        let cmd = match m {
                            ViewMode::Details => MenuCommand::LayoutDetails,
                            ViewMode::List => MenuCommand::LayoutList,
                            ViewMode::Cards => MenuCommand::LayoutCards,
                            ViewMode::Grid => MenuCommand::LayoutGrid,
                            ViewMode::Columns => MenuCommand::LayoutColumns,
                        };
                        FlyoutItem::new(tr(m.label_key()).to_string(), true)
                            .with_icon(m.icon())
                            .checked(mode == m)
                            .with_command(cmd)
                    })
                    .collect(),
            );
        let in_recycle_bin = tab.location == Location::RecycleBin;
        let mut sort_children = vec![FlyoutItem::new(tr("Name").to_string(), true)
            .checked(sort == SortColumn::Name)
            .with_command(MenuCommand::SortByName)];
        // In the recycle bin, « Date de modification » disappears: the port
        // stores the DELETION date in `modified` there (the C# keeps the
        // entry, but its SortOption.DateModified is distinct from
        // DateDeleted — a deliberate discrepancy).
        if !in_recycle_bin {
            sort_children.push(
                FlyoutItem::new(tr("DateModifiedLowerCase").to_string(), true)
                    .checked(sort == SortColumn::DateModified)
                    .with_command(MenuCommand::SortByDateModified),
            );
        }
        sort_children.extend([
            FlyoutItem::new(tr("Type").to_string(), true)
                .checked(sort == SortColumn::Type)
                .with_command(MenuCommand::SortByType),
            FlyoutItem::new(tr("Size").to_string(), true)
                .checked(sort == SortColumn::Size)
                .with_command(MenuCommand::SortBySize),
        ]);
        // `ContentPageContextFlyoutFactory.cs:193-200`: SortByOriginalFolder
        // and SortByDateDeleted, executable only in the recycle bin
        // (`SortAction.cs`: GetIsExecutable => ContentPageTypes.RecycleBin).
        if in_recycle_bin {
            sort_children.extend([
                FlyoutItem::new(tr("OriginalFolder").to_string(), true)
                    .checked(sort == SortColumn::OriginalPath)
                    .with_command(MenuCommand::SortByOriginalPath),
                FlyoutItem::new(tr("DateDeleted").to_string(), true)
                    .checked(sort == SortColumn::DateDeleted)
                    .with_command(MenuCommand::SortByDateDeleted),
            ]);
        }
        sort_children.extend([
            FlyoutItem::separator(),
            FlyoutItem::new(tr("Ascending").to_string(), true)
                .checked(ascending)
                .with_command(MenuCommand::SortAscending),
            FlyoutItem::new(tr("Descending").to_string(), true)
                .checked(!ascending)
                .with_command(MenuCommand::SortDescending),
        ]);
        let sort_menu = FlyoutItem::new(tr("SortBy").to_string(), true)
            .with_icon("Sorting")
            .with_children(sort_children);
        // `GroupBy` (`GroupBySubMenu` from the factory). The 3rd level
        // (Year/Month/Day of « Date de modification ») only exists in the
        // generic 2-level menu: here « Date de modification » applies the
        // current unit; fine granularity stays reachable via the « Trier »
        // flyout (§45).
        let grouped = group != GroupOption::None;
        let group_menu = FlyoutItem::new(tr("GroupBy").to_string(), true)
            .with_icon("Grouping")
            .with_children(vec![
                FlyoutItem::new(tr("None").to_string(), true)
                    .checked(group == GroupOption::None)
                    .with_command(MenuCommand::GroupByNone),
                FlyoutItem::new(tr("Name").to_string(), true)
                    .checked(group == GroupOption::Name)
                    .with_command(MenuCommand::GroupByName),
                FlyoutItem::new(tr("DateModifiedLowerCase").to_string(), true)
                    .checked(group == GroupOption::DateModified)
                    .with_command(MenuCommand::GroupByDateModified),
                FlyoutItem::new(tr("Type").to_string(), true)
                    .checked(group == GroupOption::Type)
                    .with_command(MenuCommand::GroupByType),
                FlyoutItem::new(tr("Size").to_string(), true)
                    .checked(group == GroupOption::Size)
                    .with_command(MenuCommand::GroupBySize),
                FlyoutItem::separator(),
                FlyoutItem::new(tr("Ascending").to_string(), grouped)
                    .checked(group_asc)
                    .with_command(MenuCommand::GroupAscending),
                FlyoutItem::new(tr("Descending").to_string(), grouped)
                    .checked(!group_asc)
                    .with_command(MenuCommand::GroupDescending),
            ]);
        // `GetNewItemItems` (ContentPageContextFlyoutFactory): Folder, File,
        // Shortcut (dialog), separator, then each ShellNew template sorted by
        // name. The port falls back to the generic document glyph for
        // templates (no base64 icon), cf. `shell_new.rs`.
        let mut new_children = vec![
            FlyoutItem::new(tr("Folder").to_string(), true)
                .with_icon("Folder")
                .with_command(MenuCommand::CreateFolder),
            FlyoutItem::new(tr("File").to_string(), true)
                .with_icon("File")
                .with_command(MenuCommand::CreateFile),
            FlyoutItem::new(tr("Shortcut").to_string(), true)
                .with_glyph("\u{E71B}")
                .with_command(MenuCommand::CreateShortcutFromDialog),
        ];
        if dir {
            let templates = crate::utils::shell_new::list_shell_new_entries();
            if !templates.is_empty() {
                new_children.push(FlyoutItem::separator());
                for (i, entry) in templates.into_iter().enumerate() {
                    new_children.push(
                        FlyoutItem::new(entry.display_name, true)
                            .with_glyph("\u{E7C3}")
                            .with_command(MenuCommand::CreateShellNew(i)),
                    );
                }
            }
        }
        let new_menu = FlyoutItem::new(tr("BaseLayoutContextFlyoutNew.Label").to_string(), dir)
            .with_icon("NewItem")
            .with_children(new_children);

        let mut items = vec![
            layout_menu,
            sort_menu,
            group_menu,
            FlyoutItem::new(tr("Refresh").to_string(), true)
                .with_icon("Refresh")
                .with_accel(hotkey_text(&["Control"], "R"))
                .with_command(MenuCommand::RefreshItems),
        ];
        // « Nouveau », Terminal and the shell menu are ABSENT from the
        // recycle bin (the factory doesn't mark these entries `ShowInRecycleBin`).
        if self.state.active().location != Location::RecycleBin {
            items.extend([
                FlyoutItem::separator(),
                new_menu,
                FlyoutItem::separator(),
                FlyoutItem::new(tr("OpenTerminal").to_string(), dir)
                    .with_glyph("\u{E756}")
                    .with_accel(hotkey_text_vk(0xC0, true, false, false))
                    .with_command(MenuCommand::OpenTerminal),
                FlyoutItem::separator(),
                FlyoutItem::new(tr("ShowMoreOptions").to_string(), dir)
                    .with_glyph("\u{E712}")
                    .with_children(vec![FlyoutItem::new(tr("Loading").to_string(), false)])
                    .with_command(MenuCommand::ShowMoreOptions),
            ]);
        }
        // `CloseActivePane`: offered only in multi-pane, like in
        // `ContentPageContextFlyoutFactory` (IsMultiPaneActive).
        if self.state.group().panes.len() == 2 {
            // After « Actualiser » (Layout, SortBy, GroupBy, Refresh = index 3).
            items.insert(
                4,
                FlyoutItem::new(tr("CloseActivePane").to_string(), true)
                    .with_glyph("\u{E89F}")
                    .with_command(MenuCommand::CloseActivePane),
            );
        }
        // Recycle bin: « Vider la corbeille » + « Restaurer tous les éléments »
        // (the factory shows them for `IsPageTypeRecycleBin && !itemsSelected`).
        if self.state.active().location == Location::RecycleBin {
            let has_items = !self.state.active().entries.is_empty();
            items.push(
                FlyoutItem::new(tr("EmptyRecycleBin").to_string(), has_items)
                    .with_icon("Delete")
                    .with_command(MenuCommand::EmptyRecycleBin),
            );
            items.push(
                // `App.ThemedIcons.RestoreDeleted`: the icon that
                // `RestoreAllRecycleBinAction.Glyph` declares.
                FlyoutItem::new(tr("RestoreAllItems").to_string(), has_items)
                    .with_icon("RestoreDeleted")
                    .with_command(MenuCommand::RestoreAllRecycleBin),
            );
        }
        // The same `IsPrimary` band, with `itemsSelected == false`: Cut, Copy,
        // Rename, Share and Delete are all hidden (unset `IsVisible` + a command
        // that is not executable without a selection), leaving the two that stay.
        let primary = vec![
            FlyoutItem::new(tr("Paste").to_string(), dir && crate::utils::storage::clipboard_has_files())
                .with_icon("Paste")
                .with_command(MenuCommand::PasteItem),
            FlyoutItem::new(tr("OpenProperties").to_string(), dir)
                .with_icon("Properties")
                .with_command(MenuCommand::OpenProperties),
        ];
        let path = match &self.state.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        };
        if let Some(dir) = path.clone() {
            self.request_shell_overflow(dir);
        }
        self.open_flyout_with_primary(
            crate::ui::FlyoutKind::EmptySpace,
            items,
            primary,
            path,
            x_px,
            y_px,
        );
    }

    /// The sidebar item's context flyout (`SidebarViewModel.GetLocationItem
    /// MenuItems`): open (tab / window / pane), copy, pin, terminal,
    /// « Afficher plus d'options » (shell) and properties.
    pub(crate) fn show_sidebar_context_menu(&mut self, path: String, x_px: f32, y_px: f32) {
        use crate::ui::{FlyoutItem, MenuCommand};
        let tr = kubuno_drive_desktop_localization::tr;
        let pinned = self.model.quick_access.iter().any(|q| q.path == path);
        // The recycle bin item (`SidebarViewModel.cs:1088-1095`, `ShowEmptyRecycleBin`).
        let is_recycle_bin = path.eq_ignore_ascii_case("shell:RecycleBinFolder");
        let mut items: Vec<FlyoutItem> = Vec::new();
        if is_recycle_bin {
            // `Commands.EmptyRecycleBin` and `Commands.RestoreAllRecycleBin`,
            // BEFORE the common entries — visible only for the recycle bin
            // and enabled per `IsExecutable` (bin not empty).
            let has_items = crate::services::storage::storage_trash_bin_service::query().1 > 0;
            items.push(
                FlyoutItem::new(tr("EmptyRecycleBin").to_string(), has_items)
                    .with_icon("Delete")
                    .with_command(MenuCommand::EmptyRecycleBin),
            );
            items.push(
                FlyoutItem::new(tr("RestoreAllItems").to_string(), has_items)
                    .with_icon("RestoreDeleted")
                    .with_command(MenuCommand::RestoreAllRecycleBin),
            );
        }
        items.extend([
            FlyoutItem::new(tr("OpenInNewTab").to_string(), true)
                .with_icon("OpenInTab")
                .with_command(MenuCommand::OpenInNewTab),
            FlyoutItem::new(tr("OpenInNewWindow").to_string(), true)
                .with_icon("OpenInWindow")
                .with_command(MenuCommand::OpenInNewWindow),
            // `OpenInNewPaneFromSidebar`: Vertical / Horizontal submenu.
            FlyoutItem::new(tr("OpenInNewPane").to_string(), true)
                .with_icon("OpenInPaneVertical")
                .with_children(vec![
                    FlyoutItem::new(tr("SplitPaneVertically").to_string(), true)
                        .with_icon("OpenInPaneVertical")
                        .with_command(MenuCommand::OpenInNewPaneVertical),
                    FlyoutItem::new(tr("SplitPaneHorizontally").to_string(), true)
                        .with_icon("OpenInPaneHorizontal")
                        .with_command(MenuCommand::OpenInNewPaneHorizontal),
                ]),
            FlyoutItem::separator(),
        ]);
        if !is_recycle_bin {
            // `CopyItemFromSidebar`: copies the folder to the clipboard —
            // not executable for the recycle bin (not a folder path).
            items.push(
                FlyoutItem::new(tr("Copy").to_string(), true)
                    .with_icon("Copy")
                    .with_command(MenuCommand::CopyItem),
            );
        }
        items.push(if pinned {
            FlyoutItem::new(tr("UnpinFolderFromSidebar").to_string(), true)
                .with_icon("FavoritePin")
                .with_command(MenuCommand::UnpinFolderFromSidebar)
        } else {
            FlyoutItem::new(tr("PinFolderToSidebar").to_string(), true)
                .with_icon("FavoritePin")
                .with_command(MenuCommand::PinFolderToSidebar)
        });
        if !is_recycle_bin {
            // Terminal and the shell menu: reserved for real folders, like in
            // the original (`OpenTerminalFromSidebar` / `ItemOverflow` don't
            // apply to `shell:RecycleBinFolder`).
            items.extend([
                FlyoutItem::separator(),
                FlyoutItem::new(tr("OpenTerminal").to_string(), true)
                    .with_glyph("\u{E756}")
                    .with_command(MenuCommand::OpenTerminal),
                // `ItemOverflow`: the shell's « Afficher plus d'options » menu.
                FlyoutItem::new(tr("ShowMoreOptions").to_string(), true)
                    .with_glyph("\u{E712}")
                    .with_children(vec![FlyoutItem::new(tr("Loading").to_string(), false)])
                    .with_command(MenuCommand::ShowMoreOptions),
            ]);
        }
        items.extend([
            FlyoutItem::separator(),
            FlyoutItem::new(tr("OpenProperties").to_string(), true)
                .with_icon("Properties")
                .with_command(MenuCommand::OpenProperties),
        ]);
        // Drive root: Eject / Storage Sense / Format (before properties),
        // like `SidebarViewModel.GetLocationItemMenuItems`.
        if is_drive_root(&path) {
            let props = items.len().saturating_sub(1); // before « Propriétés ».
            items.insert(props, FlyoutItem::separator());
            items.insert(
                props + 1,
                FlyoutItem::new(tr("Eject").to_string(), true)
                    .with_glyph("\u{E8DA}")
                    .with_command(MenuCommand::EjectDrive),
            );
            items.insert(
                props + 2,
                FlyoutItem::new(tr("OpenStorageSense").to_string(), true)
                    .with_glyph("\u{E9D9}")
                    .with_command(MenuCommand::OpenStorageSense),
            );
            if !path.trim_end_matches('\\').eq_ignore_ascii_case("C:") {
                items.insert(
                    props + 3,
                    FlyoutItem::new(tr("FormatDriveText").to_string(), true)
                        .with_glyph("\u{E954}")
                        .with_command(MenuCommand::FormatDrive),
                );
            }
        }
        // No shell submenu for the recycle bin (the « Afficher plus
        // d'options » entry is absent from its menu).
        if !is_recycle_bin {
            self.request_shell_overflow(path.clone());
        }
        self.open_flyout(crate::ui::FlyoutKind::SidebarContext, items, Some(path), x_px, y_px);
    }


    /// Live tab reordering while dragging (port of the TabBar drag & drop).
    /// Outside the strip (± `TAB_DETACH_MARGIN`), the tab DETACHES: removed
    /// from the strip (its neighbors close the gap), a ghost follows the
    /// cursor; it re-anchors if the cursor comes back — the TabView's behavior.
    /// Handles a click while the flyout is open. Returns true when consumed.
    pub(crate) fn on_flyout_click(&mut self, x: f32, y: f32) -> bool {
        use crate::ui::{FlyoutHit, FlyoutKind};
        let Some(flyout) = &self.state.flyout else {
            return false;
        };

        // The context menus built from ContentPageContextFlyoutFactory dispatch
        // by COMMAND, not by item index — their content varies with the item.
        if matches!(
            flyout.kind,
            FlyoutKind::ItemContext | FlyoutKind::EmptySpace | FlyoutKind::SidebarContext
        ) {
            let hit = flyout.hit(x, y);
            // A parent row just opens its submenu.
            if let FlyoutHit::Item(i) = hit {
                if flyout.items[i].has_submenu {
                    let flyout = self.state.flyout.as_mut().unwrap();
                    let next = if flyout.submenu == Some(i) { None } else { Some(i) };
                    flyout.set_submenu(next);
                    self.invalidate();
                    return true;
                }
            }
            let command = flyout.command_at(hit);
            let path = flyout.path.clone();
            match hit {
                FlyoutHit::Panel => {}
                _ => {
                    self.state.flyout = None;
                    if let Some(command) = command {
                        self.run_menu_command(command, path);
                    }
                    self.invalidate();
                }
            }
            return true;
        }

        // The breadcrumb ellipsis flyout: each item navigates to the
        // corresponding collapsed head segment (dispatched by index).
        if flyout.kind == FlyoutKind::BreadcrumbOverflow {
            if let FlyoutHit::Item(i) = flyout.hit(x, y) {
                let target = self.breadcrumb_overflow.get(i).cloned();
                self.state.flyout = None;
                if let Some(path) = target {
                    if !path.as_os_str().is_empty() {
                        self.navigate_active(Location::Dir(path));
                    }
                }
            } else {
                self.state.flyout = None;
            }
            self.invalidate();
            return true;
        }

        // A ComboBox's menu: the choice is handed back to `dropdown`'s modal
        // loop via `combo_pick`; any click outside the menu dismisses it.
        if flyout.kind == FlyoutKind::Combo {
            match flyout.hit(x, y) {
                FlyoutHit::Item(i) => {
                    if flyout.items[i].enabled {
                        self.combo_pick = Some(i);
                        self.state.flyout = None;
                    }
                }
                FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        if let FlyoutKind::TabContext(tab) = flyout.kind {
            match flyout.hit(x, y) {
                FlyoutHit::Item(i) => {
                    let enabled = flyout.items[i].enabled;
                    self.state.flyout = None;
                    if enabled {
                        self.run_tab_context_command(tab, i + 1);
                    }
                    self.invalidate();
                }
                FlyoutHit::Outside => {
                    self.state.flyout = None;
                    self.invalidate();
                }
                _ => {}
            }
            return true;
        }

        if flyout.kind == FlyoutKind::NewMenu {
            match flyout.hit(x, y) {
                FlyoutHit::Item(i) => {
                    self.state.flyout = None;
                    if let Location::Dir(dir) = &self.state.active().location {
                        let dir = dir.to_string_lossy().into_owned();
                        self.create_item(&dir, i == 0);
                    }
                }
                FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        if flyout.kind == FlyoutKind::SelectionMenu {
            use crate::actions::Action;
            match flyout.hit(x, y) {
                FlyoutHit::Item(i @ (0..=2)) => {
                    self.state.flyout = None;
                    let action: &dyn Action = match i {
                        0 => &crate::actions::content::selection::SelectAll,
                        1 => &crate::actions::content::selection::InvertSelection,
                        _ => &crate::actions::content::selection::ClearSelection,
                    };
                    action.execute(self, None);
                }
                FlyoutHit::Item(_) | FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        if flyout.kind == FlyoutKind::SortMenu {
            use crate::actions::display::group_action as ga;
            use crate::actions::display::sort_files_first_action as sf;
            use crate::actions::Action;
            // Root: [0] Sort by ▸, [1] Group by ▸, [2] sep., [3..5] trio.
            let hit = flyout.hit(x, y);
            match hit {
                // A parent (Sort by / Group by) toggles its submenu.
                FlyoutHit::Item(i @ (0 | 1)) => {
                    let flyout = self.state.flyout.as_mut().unwrap();
                    let next = if flyout.submenu == Some(i) { None } else { Some(i) };
                    flyout.set_submenu(next);
                }
                // The folders/files radio trio.
                FlyoutHit::Item(i @ (3..=5)) => {
                    self.state.flyout = None;
                    let action: &dyn Action = match i {
                        3 => &sf::SortFoldersFirst,
                        4 => &sf::SortFilesFirst,
                        _ => &sf::SortFilesAndFoldersTogether,
                    };
                    action.execute(self, None);
                }
                // Level 2: depends on which submenu is open.
                FlyoutHit::SubItem(k) => match self.state.flyout.as_ref().and_then(|f| f.submenu) {
                    // « Trier par »: columns + direction, by command.
                    Some(0) => {
                        let cmd = self.state.flyout.as_ref().and_then(|f| f.command_at(hit));
                        self.state.flyout = None;
                        if let Some(cmd) = cmd {
                            self.run_menu_command(cmd, None);
                        }
                    }
                    // « Grouper par »: k==2 (Date modified) unfolds the 3rd
                    // level; otherwise apply the option/direction.
                    Some(1) => {
                        if k == 2 {
                            let flyout = self.state.flyout.as_mut().unwrap();
                            let next = if flyout.subsubmenu == Some(2) { None } else { Some(2) };
                            flyout.set_subsubmenu(next);
                        } else {
                            self.state.flyout = None;
                            let action: Option<&dyn Action> = match k {
                                0 => Some(&ga::GroupByNone),
                                1 => Some(&ga::GroupByName),
                                3 => Some(&ga::GroupByType),
                                4 => Some(&ga::GroupBySize),
                                6 => Some(&ga::GroupAscending),
                                7 => Some(&ga::GroupDescending),
                                _ => None,
                            };
                            if let Some(a) = action {
                                if a.is_executable(self) {
                                    a.execute(self, None);
                                }
                            }
                        }
                    }
                    _ => {}
                },
                // Level 3: Year / Month / Day.
                FlyoutHit::SubSubItem(k) => {
                    self.state.flyout = None;
                    let action: &dyn Action = match k {
                        0 => &ga::GroupByDateModifiedYear,
                        1 => &ga::GroupByDateModifiedMonth,
                        _ => &ga::GroupByDateModifiedDay,
                    };
                    action.execute(self, None);
                }
                FlyoutHit::Item(_) | FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        // Git network actions: Pull / Push / Sync (indices 0/1/2).
        if flyout.kind == FlyoutKind::GitActions {
            match flyout.hit(x, y) {
                FlyoutHit::Item(i @ (0..=2)) => {
                    self.state.flyout = None;
                    if let Some(dir) = self.git_dir() {
                        self.run_git_op(move || match i {
                            0 => crate::utils::git::pull(&dir),
                            1 => crate::utils::git::push(&dir),
                            _ => crate::utils::git::sync(&dir),
                        });
                    }
                }
                FlyoutHit::Item(_) | FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        // Branch picker: clicking a branch checks it out; the last item
        // (after the separator) opens the « Créer une branche » dialog.
        if flyout.kind == FlyoutKind::GitBranches {
            match flyout.hit(x, y) {
                FlyoutHit::Item(i) => {
                    let n = self.state.git_branches.len();
                    self.state.flyout = None;
                    if i < n {
                        // Checkout branch i (unless it's already HEAD).
                        if !self.state.git_branches[i].is_head {
                            let name = self.state.git_branches[i].name.clone();
                            if let Some(dir) = self.git_dir() {
                                self.run_git_op(move || crate::utils::git::checkout(&dir, &name));
                            }
                        }
                    } else {
                        // The « Créer une branche » item (after the separator).
                        self.open_create_branch_dialog();
                    }
                }
                FlyoutHit::Outside => self.state.flyout = None,
                _ => {}
            }
            self.invalidate();
            return true;
        }

        // The ColorPicker: like the Layout panel, it stays open as long as
        // you're acting inside it; every gesture applies the color LIVE.
        if flyout.kind == FlyoutKind::ColorPicker {
            use crate::user_controls::color_picker::PickerZone;
            let Some(panel) = flyout.picker else {
                self.state.flyout = None;
                self.invalidate();
                return true;
            };
            let (px, py) = (flyout.x, flyout.y);
            if !flyout.panel_rect().contains(x, y) {
                self.state.flyout = None;
                self.invalidate();
                return true;
            }
            if let Some(zone) = panel.hit(px, py, x, y) {
                // Continuous zones are handled on button-DOWN (dragging); the
                // click handles the discrete ones — the chips and the close
                // button of the panel's header.
                if zone == PickerZone::Close {
                    self.state.flyout = None;
                    self.invalidate();
                    return true;
                }
                if matches!(zone, PickerZone::Click(_)) {
                    let mut p = panel;
                    p.apply(zone, px, py, x, y);
                    if let Some(f) = self.state.flyout.as_mut() {
                        f.picker = Some(p);
                    }
                    self.apply_picker_color(&p);
                }
            }
            self.invalidate();
            return true;
        }

        // The « Disposition » panel: cards, slider, toggles. It stays open
        // while you act inside it (it's a Flyout, not a menu: only a click
        // outside closes it).
        if flyout.kind == FlyoutKind::ViewMode {
            use crate::user_controls::layout_flyout::LayoutHit;
            use crate::view_models::shell_view_model::ViewMode;
            let Some(panel) = flyout.layout else {
                self.state.flyout = None;
                self.invalidate();
                return true;
            };
            let (px, py) = (flyout.x, flyout.y);
            if !flyout.panel_rect().contains(x, y) {
                self.state.flyout = None;
                self.invalidate();
                return true;
            }
            match panel.hit(px, py, x, y) {
                LayoutHit::Card(i) => {
                    let mode = ViewMode::ALL[i];
                    self.set_view_mode(mode);
                    let size = self.layout_size(mode);
                    if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                        p.mode = mode;
                        p.size = size;
                    }
                }
                LayoutHit::Slider => {
                    let size = panel.size_at(px, py, x);
                    self.set_layout_size(panel.mode, size);
                    if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                        p.size = size;
                        p.dragging = true;
                    }
                }
                LayoutHit::ToggleHidden => {
                    use crate::actions::Action;
                    crate::actions::show::toggle_show_hidden_items_action::ToggleShowHiddenItems
                        .execute(self, None);
                    let on = crate::services::settings::get().show_hidden_items;
                    if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                        p.show_hidden = on;
                    }
                }
                LayoutHit::ToggleExtensions => {
                    use crate::actions::Action;
                    crate::actions::show::toggle_show_file_extensions_action::ToggleShowFileExtensions
                        .execute(self, None);
                    let on = crate::services::settings::get().show_file_extensions;
                    if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                        p.show_extensions = on;
                    }
                }
                LayoutHit::Panel => {}
            }
            self.invalidate();
            return true;
        }

        match flyout.hit(x, y) {
            FlyoutHit::Item(0) => {
                self.state.flyout = None;
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            FlyoutHit::Item(1) => {
                self.state.flyout = None;
                self.toggle_compact_overlay();
            }
            FlyoutHit::Item(2) => {
                // Toggle the "Diviser le volet" submenu (its entries live on
                // the item itself, like ContextMenuFlyoutItemViewModel.Items).
                let flyout = self.state.flyout.as_mut().unwrap();
                let next = if flyout.submenu.is_some() { None } else { Some(2) };
                flyout.set_submenu(next);
            }
            FlyoutHit::SubItem(i) => {
                // Split pane: Vertical (side by side) / Horizontal.
                self.state.flyout = None;
                self.state.group_mut().split(i == 0);
            }
            FlyoutHit::Item(3) => {
                // `CloseActivePane` (only present in multi-pane).
                self.state.flyout = None;
                self.state.group_mut().close_active_pane();
            }
            FlyoutHit::Item(_) | FlyoutHit::SubSubItem(_) | FlyoutHit::Primary(_) | FlyoutHit::Panel => {}
            FlyoutHit::Outside => {
                self.state.flyout = None;
            }
        }
        self.invalidate();
        true
    }

}
