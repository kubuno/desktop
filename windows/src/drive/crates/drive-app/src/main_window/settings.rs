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
    pub(crate) fn apply_picker_color(&mut self, panel: &crate::user_controls::color_picker::ColorPickerPanel) {
        let hex = panel.hex();
        crate::services::settings::update(|s| s.app_theme_background_color = hex);
        self.theme = crate::styles::theme::from_settings();
        self.invalidate();
    }

    pub(crate) fn on_setting_row(&mut self, row: usize, x_px: f32, y_px: f32) {
        if self.state.settings_section == 1 {
            self.on_appearance_row(row, x_px, y_px);
            return;
        }
        let layout = self.layout();
        let Some(page) = &layout.settings_page else { return };
        let Some(srow) = page.rows.get(row) else { return };
        let rect = page.rects[row];
        let id = srow.id;
        let kind = srow.kind;
        let control = srow.control.clone();
        self.on_settings_row_id(id, kind, control, rect, x_px, y_px);
    }

    /// Toggles a SettingsExpander of the current section.
    pub(crate) fn toggle_settings_expander(&mut self, exp: usize) {
        let section = self.state.settings_section;
        self.state.settings_expanded[section][exp] ^= true;
        self.invalidate();
    }

    /// Simple persisted toggle + redraw.
    pub(crate) fn toggle_setting(&mut self, mutate: impl FnOnce(&mut crate::services::settings::AppSettings)) {
        crate::services::settings::update(mutate);
        self.invalidate();
    }

    /// Refreshes every open directory pane (after ShowHiddenItems…).
    pub(crate) fn refresh_all_dirs(&mut self) {
        for group in &mut self.state.tabs {
            for pane in &mut group.panes {
                if matches!(pane.location, Location::Dir(_)) {
                    pane.refresh();
                }
            }
        }
        self.invalidate();
    }

    /// Click dispatch for the generic settings pages (all sections except
    /// Appearance), mirroring each page's ViewModel actions.
    pub(crate) fn on_settings_row_id(
        &mut self,
        id: crate::views::settings::controls::SettingId,
        kind: crate::views::settings::controls::RowKind,
        control: crate::views::settings::controls::Control,
        rect: crate::ui::Rect,
        x_px: f32,
        y_px: f32,
    ) {
        use crate::services::settings::{
            DateTimeFormat, DefaultGroupOption, DefaultSortOption, DeleteConfirmationPolicy,
            FolderLayoutMode, GroupByDateUnit, OpenInIDEOption, ShellPaneArrangement,
            SingleClickOpenMode, SizeUnitFormat,
        };
        use crate::views::settings::controls::{self as sc, Control, RowKind, SettingId};
        use crate::views::settings::{
            about_page, dev_tools_page, folders_page, general_page, layout_page,
        };
        let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));

        // The settings controls are MEASURED by their `kubuno-ui` primitives
        // against the real font, so the hit test has to measure the same way
        // the paint did: a `Painter` built purely to measure (nothing is drawn
        // through it, and it is dropped before `self` is touched again).
        //
        // A ComboBox drops its menu BELOW the button, left-aligned — the anchor
        // is computed here for the `dropdown` that the arm will open. Expander
        // headers: only their own control zone is NOT an expand toggle (like
        // the SettingsExpander in the original).
        let header = matches!(kind, RowKind::ExpanderHeader { .. });
        let expander = match kind {
            RowKind::ExpanderHeader { exp, .. } => Some(exp),
            _ => None,
        };
        let (anchor, expand) = {
            let measure = self
                .renderer
                .as_ref()
                .and_then(|r| crate::ui::Painter::new(r, &self.theme).ok());
            match measure.as_ref() {
                Some(p) => {
                    let anchor = match &control {
                        Control::Combo(v) | Control::ComboDisabled(v) => {
                            Some(sc::combo_rect(p, &rect, header, v))
                        }
                        _ => None,
                    };
                    let in_control = match &control {
                        Control::Combo(v) | Control::ComboDisabled(v) => {
                            sc::combo_rect(p, &rect, true, v).contains(x, y)
                        }
                        Control::Toggle(_) | Control::ToggleDisabled(_) => {
                            sc::header_toggle_rect(&rect).contains(x, y)
                        }
                        Control::Button(text) => sc::button_rect(p, &rect, text, true).contains(x, y),
                        _ => false,
                    };
                    (anchor, expander.filter(|_| !in_control))
                }
                // No render target (between a device loss and the next paint):
                // nothing can be measured, so the header behaves as if the
                // click missed every control — which is what it did before.
                None => (None, expander),
            }
        };
        if anchor.is_some() {
            self.combo_anchor = anchor;
        }
        if let Some(exp) = expand {
            self.toggle_settings_expander(exp);
            return;
        }

        match id {
            SettingId::None => {}

            // === GeneralPage ===
            // The language follows the system (the picker shows the value).
            SettingId::GenLanguage => {}
            SettingId::GenDateFormat => {
                let current = crate::services::settings::get().date_time_format;
                let items = general_page::date_format_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| DateTimeFormat::ALL.get(i))
                {
                    self.toggle_setting(|s| s.date_time_format = *v);
                }
            }
            SettingId::GenStartupHeader => {
                let s = crate::services::settings::get();
                let items = general_page::startup_options(&s);
                match self.dropdown(&items, x_px, y_px) {
                    1 => self.toggle_setting(|s| {
                        s.open_new_tab_on_startup = true;
                        s.continue_last_session_on_startup = false;
                        s.open_specific_page_on_startup = false;
                    }),
                    2 => self.toggle_setting(|s| {
                        s.open_new_tab_on_startup = false;
                        s.continue_last_session_on_startup = true;
                        s.open_specific_page_on_startup = false;
                    }),
                    3 => self.toggle_setting(|s| {
                        s.open_new_tab_on_startup = false;
                        s.continue_last_session_on_startup = false;
                        s.open_specific_page_on_startup = true;
                    }),
                    _ => {}
                }
            }
            SettingId::GenStartupPages => {
                let tr = drive_localization::tr;
                if general_page::add_page_button_rect(&rect).contains(x, y) {
                    // MenuFlyout: Accueil / Parcourir (AddPageCommand).
                    match self.dropdown(&[(tr("Home"), false), (tr("Browse"), false)], x_px, y_px) {
                        1 => self.toggle_setting(|s| s.tabs_on_startup_list.push("Home".into())),
                        2 => {
                            if let Some(path) = pick_folder(self.hwnd) {
                                self.toggle_setting(|s| s.tabs_on_startup_list.push(path));
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                let count = crate::services::settings::get().tabs_on_startup_list.len();
                for i in 0..count {
                    let row = general_page::startup_page_row_rect(&rect, i);
                    if general_page::remove_page_button_rect(&row).contains(x, y) {
                        self.toggle_setting(|s| {
                            s.tabs_on_startup_list.remove(i);
                        });
                        return;
                    }
                }
            }
            SettingId::GenOpenTabInExistingInstance => {
                self.toggle_setting(|s| s.open_tab_in_existing_instance = !s.open_tab_in_existing_instance)
            }
            SettingId::GenAlwaysSwitchToNewlyOpenedTab => {
                self.toggle_setting(|s| s.always_switch_to_newly_opened_tab = !s.always_switch_to_newly_opened_tab)
            }
            SettingId::GenWidgetsHeader | SettingId::GenContextMenuHeader => {}
            SettingId::GenWidgetQuickAccess => {
                self.toggle_setting(|s| s.show_quick_access_widget = !s.show_quick_access_widget)
            }
            SettingId::GenWidgetDrives => {
                self.toggle_setting(|s| s.show_drives_widget = !s.show_drives_widget)
            }
            SettingId::GenWidgetNetworkLocations => {
                self.toggle_setting(|s| s.show_network_locations_widget = !s.show_network_locations_widget)
            }
            SettingId::GenWidgetFileTags => {
                self.toggle_setting(|s| s.show_file_tags_widget = !s.show_file_tags_widget)
            }
            SettingId::GenWidgetRecentFiles => {
                self.toggle_setting(|s| s.show_recent_files_widget = !s.show_recent_files_widget)
            }
            SettingId::GenAlwaysOpenDualPane => {
                self.toggle_setting(|s| s.always_open_dual_pane_in_new_tab = !s.always_open_dual_pane_in_new_tab)
            }
            SettingId::GenDualPaneArrangement => {
                let current = crate::services::settings::get().shell_pane_arrangement;
                let items = general_page::arrangement_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| ShellPaneArrangement::ALL.get(i))
                {
                    self.toggle_setting(|s| s.shell_pane_arrangement = *v);
                }
            }
            SettingId::GenCtxOpenInNewTab => {
                self.toggle_setting(|s| s.show_open_in_new_tab = !s.show_open_in_new_tab)
            }
            SettingId::GenCtxOpenInNewWindow => {
                self.toggle_setting(|s| s.show_open_in_new_window = !s.show_open_in_new_window)
            }
            SettingId::GenCtxOpenInNewPane => {
                self.toggle_setting(|s| s.show_open_in_new_pane = !s.show_open_in_new_pane)
            }
            SettingId::GenCtxCopyPath => self.toggle_setting(|s| s.show_copy_path = !s.show_copy_path),
            SettingId::GenCtxCreateFolderWithSelection => {
                self.toggle_setting(|s| s.show_create_folder_with_selection = !s.show_create_folder_with_selection)
            }
            SettingId::GenCtxCreateAlternateDataStream => {
                self.toggle_setting(|s| s.show_create_alternate_data_stream = !s.show_create_alternate_data_stream)
            }
            SettingId::GenCtxCreateShortcut => {
                self.toggle_setting(|s| s.show_create_shortcut = !s.show_create_shortcut)
            }
            SettingId::GenCtxPinToSideBar => {
                self.toggle_setting(|s| s.show_pin_to_sidebar = !s.show_pin_to_sidebar)
            }
            SettingId::GenCtxCompressionOptions => {
                self.toggle_setting(|s| s.show_compression_options = !s.show_compression_options)
            }
            SettingId::GenCtxSendToMenu => self.toggle_setting(|s| s.show_send_to_menu = !s.show_send_to_menu),
            SettingId::GenCtxOpenTerminal => {
                self.toggle_setting(|s| s.show_open_terminal = !s.show_open_terminal)
            }
            SettingId::GenCtxEditTagsMenu => {
                self.toggle_setting(|s| s.show_edit_tags_menu = !s.show_edit_tags_menu)
            }
            SettingId::GenCtxPinToStart => self.toggle_setting(|s| s.show_pin_to_start = !s.show_pin_to_start),
            SettingId::GenCtxOverflow => {
                self.toggle_setting(|s| s.move_shell_extensions_to_sub_menu = !s.move_shell_extensions_to_sub_menu)
            }
            SettingId::GenSmoothScrolling => {
                self.toggle_setting(|s| s.enable_smooth_scrolling = !s.enable_smooth_scrolling)
            }
            SettingId::GenTabScrollDirection => {
                let tr = drive_localization::tr;
                let current = crate::services::settings::get().reverse_tab_scroll_direction;
                let items = [
                    (tr("DownMotionScrollsLeft"), !current),
                    (tr("DownMotionScrollsRight"), current),
                ];
                match self.dropdown(&items, x_px, y_px) {
                    1 => self.toggle_setting(|s| s.reverse_tab_scroll_direction = false),
                    2 => self.toggle_setting(|s| s.reverse_tab_scroll_direction = true),
                    _ => {}
                }
            }

            // === LayoutPage ===
            SettingId::LaySyncPreferences => {
                // LayoutViewModel: enabling sync while Adaptive is the
                // default layout falls back to Details.
                self.toggle_setting(|s| {
                    s.sync_folder_preferences_across_directories = !s.sync_folder_preferences_across_directories;
                    if s.sync_folder_preferences_across_directories
                        && s.default_layout_mode == FolderLayoutMode::Adaptive
                    {
                        s.default_layout_mode = FolderLayoutMode::Details;
                    }
                })
            }
            SettingId::LayLayoutType => {
                let s = crate::services::settings::get();
                let items = layout_page::layout_mode_options(&s);
                if let Some(v) = self
                    .dropdown_checked_ex(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| FolderLayoutMode::ALL.get(i))
                {
                    self.toggle_setting(|s| s.default_layout_mode = *v);
                }
            }
            SettingId::LaySortByHeader => {
                let current = crate::services::settings::get().default_sort_option;
                let items = layout_page::sort_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| DefaultSortOption::ALL.get(i))
                {
                    self.toggle_setting(|s| s.default_sort_option = *v);
                }
            }
            SettingId::LaySortDescending => {
                self.toggle_setting(|s| s.default_sort_descending = !s.default_sort_descending)
            }
            SettingId::LaySortPriority => {
                let s = crate::services::settings::get();
                let current = layout_page::sort_priority_index(&s);
                let items: Vec<(&str, bool)> = layout_page::SORT_PRIORITY_KEYS
                    .iter()
                    .enumerate()
                    .map(|(i, k)| (drive_localization::tr(k), i == current))
                    .collect();
                match self.dropdown(&items, x_px, y_px) {
                    1 => self.toggle_setting(|s| {
                        s.default_sort_directories_alongside_files = false;
                        s.default_sort_files_first = false;
                    }),
                    2 => self.toggle_setting(|s| {
                        s.default_sort_directories_alongside_files = false;
                        s.default_sort_files_first = true;
                    }),
                    3 => self.toggle_setting(|s| s.default_sort_directories_alongside_files = true),
                    _ => {}
                }
            }
            SettingId::LayGroupByHeader => {
                let current = crate::services::settings::get().default_group_option;
                let items = layout_page::group_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| DefaultGroupOption::ALL.get(i))
                {
                    self.toggle_setting(|s| s.default_group_option = *v);
                }
            }
            SettingId::LayGroupDescending => {
                if matches!(control, Control::Toggle(_)) {
                    self.toggle_setting(|s| s.default_group_descending = !s.default_group_descending)
                }
            }
            SettingId::LayGroupByDateUnit => {
                if matches!(control, Control::Combo(_)) {
                    let current = crate::services::settings::get().default_group_by_date_unit;
                    let items = layout_page::group_date_unit_options(current);
                    if let Some(v) = self
                        .dropdown(&items, x_px, y_px)
                        .checked_sub(1)
                        .and_then(|i| GroupByDateUnit::ALL.get(i))
                    {
                        self.toggle_setting(|s| s.default_group_by_date_unit = *v);
                    }
                }
            }
            SettingId::LayAutoSizeColumns => {
                self.toggle_setting(|s| s.auto_size_columns_in_details_layout = !s.auto_size_columns_in_details_layout)
            }
            SettingId::LayColumnsHeader => {}
            SettingId::LayColTag => self.toggle_setting(|s| s.show_file_tag_column = !s.show_file_tag_column),
            SettingId::LayColSize => self.toggle_setting(|s| s.show_size_column = !s.show_size_column),
            SettingId::LayColType => self.toggle_setting(|s| s.show_type_column = !s.show_type_column),
            SettingId::LayColDate => self.toggle_setting(|s| s.show_date_column = !s.show_date_column),
            SettingId::LayColDateCreated => {
                self.toggle_setting(|s| s.show_date_created_column = !s.show_date_created_column)
            }

            // === FoldersPage ===
            SettingId::FolHiddenItemsHeader | SettingId::FolSingleClickHeader => {}
            SettingId::FolShowHiddenItems => {
                crate::services::settings::update(|s| s.show_hidden_items = !s.show_hidden_items);
                self.refresh_all_dirs();
            }
            SettingId::FolShowDotFiles => self.toggle_setting(|s| s.show_dot_files = !s.show_dot_files),
            SettingId::FolShowProtectedSystemFiles => {
                crate::services::settings::update(|s| s.show_protected_system_files = !s.show_protected_system_files);
                self.refresh_all_dirs();
            }
            SettingId::FolShowAlternateStreams => {
                self.toggle_setting(|s| s.are_alternate_streams_visible = !s.are_alternate_streams_visible)
            }
            SettingId::FolShowFileExtensions => {
                self.toggle_setting(|s| s.show_file_extensions = !s.show_file_extensions)
            }
            SettingId::FolShowThumbnails => {
                self.toggle_setting(|s| s.show_thumbnails = !s.show_thumbnails)
            }
            SettingId::FolShowCheckboxes => {
                self.toggle_setting(|s| s.show_checkboxes_when_selecting_items = !s.show_checkboxes_when_selecting_items)
            }
            SettingId::FolSingleClickFiles => {
                let current = crate::services::settings::get().open_files_with_single_click;
                let items = folders_page::single_click_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| SingleClickOpenMode::ALL.get(i))
                {
                    self.toggle_setting(|s| s.open_files_with_single_click = *v);
                }
            }
            SettingId::FolSingleClickFolders => {
                let current = crate::services::settings::get().open_folders_with_single_click;
                let items = folders_page::single_click_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| SingleClickOpenMode::ALL.get(i))
                {
                    self.toggle_setting(|s| s.open_folders_with_single_click = *v);
                }
            }
            SettingId::FolSingleClickColumnsView => {
                let current = crate::services::settings::get().open_folders_in_columns_view_with_single_click;
                let items = folders_page::single_click_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| SingleClickOpenMode::ALL.get(i))
                {
                    self.toggle_setting(|s| s.open_folders_in_columns_view_with_single_click = *v);
                }
            }
            SettingId::FolOpenFoldersNewTab => {
                self.toggle_setting(|s| s.open_folders_in_new_tab = !s.open_folders_in_new_tab)
            }
            SettingId::FolDeleteConfirmation => {
                let current = crate::services::settings::get().delete_confirmation_policy;
                let items = folders_page::delete_confirmation_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| DeleteConfirmationPolicy::ALL.get(i))
                {
                    self.toggle_setting(|s| s.delete_confirmation_policy = *v);
                }
            }
            SettingId::FolExtensionWarning => {
                self.toggle_setting(|s| s.show_file_extension_warning = !s.show_file_extension_warning)
            }
            SettingId::FolSelectOnHover => {
                self.toggle_setting(|s| s.select_files_on_hover = !s.select_files_on_hover)
            }
            SettingId::FolDoubleClickToGoUp => {
                self.toggle_setting(|s| s.double_click_to_go_up = !s.double_click_to_go_up)
            }
            SettingId::FolScrollToPreviousFolder => {
                self.toggle_setting(|s| {
                    s.scroll_to_previous_folder_when_navigating_up = !s.scroll_to_previous_folder_when_navigating_up
                })
            }
            SettingId::FolSizeFormat => {
                let current = crate::services::settings::get().size_unit_format;
                let items = folders_page::size_format_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| SizeUnitFormat::ALL.get(i))
                {
                    self.toggle_setting(|s| s.size_unit_format = *v);
                }
            }
            SettingId::FolCalculateFolderSizesHeader => {
                self.toggle_setting(|s| s.calculate_folder_sizes = !s.calculate_folder_sizes);
                // `UserSizeProvider`: cache purged on toggle; folders
                // go back to "unknown size" when turned off.
                self.size_provider.clear();
                for group in &mut self.state.tabs {
                    for entry in group.active_mut().entries.iter_mut().filter(|e| e.is_dir) {
                        entry.size_known = false;
                    }
                }
            }

            // === ActionsPage (editing key bindings not ported yet) ===
            SettingId::ActTopBar | SettingId::ActCommand(_) => {}

            // === TagsPage (tag editor not ported yet) ===
            SettingId::TagsHeader | SettingId::TagRow(_) => {}

            // === DevToolsPage ===
            SettingId::DevOpenIdeHeader => {
                let current = crate::services::settings::get().open_in_ide_option;
                let items = dev_tools_page::open_in_ide_options(current);
                if let Some(v) = self
                    .dropdown(&items, x_px, y_px)
                    .checked_sub(1)
                    .and_then(|i| OpenInIDEOption::ALL.get(i))
                {
                    self.toggle_setting(|s| s.open_in_ide_option = *v);
                }
            }
            SettingId::DevIdeName | SettingId::DevIdePath | SettingId::DevGitHub => {}

            // === AdvancedPage ===
            SettingId::AdvExportSettings => {
                if let Some(path) = pick_save_json(self.hwnd) {
                    let settings = crate::services::settings::get();
                    match serde_json::to_string_pretty(&settings) {
                        Ok(json) => {
                            if let Err(e) = std::fs::write(&path, json) {
                                tracing::warn!("export settings failed: {e}");
                            }
                        }
                        Err(e) => tracing::warn!("export settings failed: {e}"),
                    }
                }
            }
            SettingId::AdvImportSettings => {
                if let Some(path) = pick_open_json(self.hwnd) {
                    match std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|json| serde_json::from_str::<crate::services::settings::AppSettings>(&json).map_err(|e| e.to_string()))
                    {
                        Ok(imported) => {
                            crate::services::settings::update(|s| *s = imported);
                            self.apply_appearance();
                            self.refresh_all_dirs();
                        }
                        Err(e) => tracing::warn!("import settings failed: {e}"),
                    }
                }
            }
            SettingId::AdvEditSettingsFile => {
                shell_open(&crate::services::settings::settings_file_path().to_string_lossy());
            }
            SettingId::AdvOpenOnStartup => {
                self.toggle_setting(|s| s.open_on_windows_startup = !s.open_on_windows_startup)
            }
            SettingId::AdvLeaveAppRunning => {
                self.toggle_setting(|s| s.leave_app_running = !s.leave_app_running)
            }
            SettingId::AdvSystemTrayIcon => {
                self.toggle_setting(|s| s.show_system_tray_icon = !s.show_system_tray_icon)
            }
            SettingId::AdvSetAsDefaultFileManager => {
                self.toggle_setting(|s| s.is_set_as_default_file_manager = !s.is_set_as_default_file_manager)
            }
            SettingId::AdvFlattenOptions => {
                self.toggle_setting(|s| s.show_flatten_options = !s.show_flatten_options)
            }

            // === AboutPage ===
            SettingId::AbtAppInfo => {
                let tr = drive_localization::tr;
                // Copy flyout: AppVersion / WindowsVersion / UserID.
                let items = [
                    (tr("AppVersion"), false),
                    (tr("WindowsVersion"), false),
                    (tr("UserID"), false),
                ];
                let text = match self.dropdown(&items, x_px, y_px) {
                    1 => about_page::APP_VERSION.to_string(),
                    2 => windows_version(),
                    3 => crate::services::settings::user_id(),
                    _ => return,
                };
                crate::utils::storage::clipboard_set_text(self.hwnd, &text);
            }
            SettingId::AbtSponsor => shell_open(about_page::SUPPORT_US_URL),
            SettingId::AbtDocumentation => shell_open(about_page::DOCUMENTATION_URL),
            SettingId::AbtDiscussions => shell_open(about_page::DISCORD_URL),
            SettingId::AbtFeedbackHeader | SettingId::AbtLibrariesHeader => {}
            SettingId::AbtFeatureRequest => shell_open(about_page::FEATURE_REQUEST_URL),
            SettingId::AbtBugReport => {
                // AboutViewModel appends files/windows versions and user id.
                let url = format!(
                    "{}&files_version={}&windows_version={}&user_id={}",
                    about_page::BUG_REPORT_URL,
                    about_page::APP_VERSION,
                    windows_version(),
                    crate::services::settings::user_id(),
                );
                shell_open(&url);
            }
            SettingId::AbtLogLocation => {
                // Launcher.LaunchFolderAsync(LocalFolder): our app data dir.
                if let Some(dir) = crate::services::settings::settings_file_path().parent() {
                    shell_open(&dir.to_string_lossy());
                }
            }
            SettingId::AbtTranslate => shell_open(about_page::CROWDIN_URL),
            SettingId::AbtLibrariesPanel => {
                for (i, (url, _)) in about_page::OPEN_SOURCE_LIBRARIES.iter().enumerate() {
                    if about_page::library_cell_rect(&rect, i).contains(x, y) {
                        shell_open(url);
                        return;
                    }
                }
            }
            SettingId::AbtGitHubRepo => shell_open(about_page::GITHUB_REPO_URL),
            SettingId::AbtPrivacy => shell_open(about_page::PRIVACY_POLICY_URL),
        }
    }

}
