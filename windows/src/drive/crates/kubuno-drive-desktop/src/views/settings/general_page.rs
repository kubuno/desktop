//! Port of `Files.App/Views/Settings/GeneralPage.xaml` + `GeneralViewModel`:
//! language, date format, startup settings, tab behavior, widgets, dual pane,
//! right-click context menu options and scrolling.

use kubuno_drive_desktop_app_controls::themes::shape::{height, radius};

use crate::services::settings::{AppSettings, DateTimeFormat, ShellPaneArrangement};
use crate::ui::{Painter, Rect};
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander indices in `UiState::settings_expanded[SECTION_GENERAL]`.
pub const EXP_STARTUP: usize = 0;
pub const EXP_WIDGETS: usize = 1;
pub const EXP_CONTEXT_MENU: usize = 2;

/// Startup combo label (`GeneralViewModel.SelectedStartupSettingIndex`).
pub fn startup_setting_label(s: &AppSettings) -> &'static str {
    let tr = kubuno_drive_desktop_localization::tr;
    if s.continue_last_session_on_startup {
        tr("SettingsOnStartupContinueWhereYouLeftOff.Content")
    } else if s.open_specific_page_on_startup {
        tr("SettingsOnStartupOpenASpecificPage.Content")
    } else {
        tr("SettingsOnStartupOpenANewTab.Content")
    }
}

/// Height of the "pages on startup" panel (ItemsHeader of StartupSettings).
pub fn startup_pages_panel_height(s: &AppSettings) -> f32 {
    44.0 + s.tabs_on_startup_list.len() as f32 * 30.0 + 8.0
}

/// The "Ajouter une page" button rect inside the panel.
/// `@ui/Button size="sm"`: h-8.
pub fn add_page_button_rect(panel: &Rect) -> Rect {
    let top = panel.top + 8.0;
    Rect::new(panel.left + 20.0, top, panel.left + 190.0, top + height::BUTTON_SM)
}

/// The startup page rows (one per path) + their remove buttons.
pub fn startup_page_row_rect(panel: &Rect, i: usize) -> Rect {
    let top = panel.top + 48.0 + i as f32 * 30.0;
    Rect::new(panel.left + 20.0, top, panel.right - 20.0, top + 28.0)
}

pub fn remove_page_button_rect(row: &Rect) -> Rect {
    Rect::new(row.right - 32.0, row.top, row.right, row.bottom)
}

pub fn rows(s: &AppSettings, expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = kubuno_drive_desktop_localization::tr;
    let mut rows = Vec::new();

    // Language settings.
    rows.push(SettingsRow::card(
        SettingId::GenLanguage,
        Icon::Glyph("\u{F2B7}"),
        tr("Language"),
        Control::Combo(crate::services::date_time_formatter::current_language_name()),
    ));

    // Date settings (Description = DateFormatSample).
    rows.push(
        SettingsRow::card(
            SettingId::GenDateFormat,
            Icon::Glyph("\u{EC92}"),
            tr("DateFormat"),
            Control::Combo(tr(s.date_time_format.tr_key()).into()),
        )
        .with_description(crate::services::date_time_formatter::date_format_sample(s.date_time_format)),
    );

    // Startup settings (SettingsExpander, IsExpanded=False).
    rows.push(SettingsRow::expander(
        SettingId::GenStartupHeader,
        Icon::Glyph("\u{E7E8}"),
        tr("StartupSettings"),
        Control::Combo(startup_setting_label(s).into()),
        EXP_STARTUP,
        expanded[EXP_STARTUP],
    ));
    if expanded[EXP_STARTUP] {
        // ItemsHeader: pages list, only when "Ouvrir une page spécifique".
        if s.open_specific_page_on_startup {
            rows.push(SettingsRow::custom(
                SettingId::GenStartupPages,
                startup_pages_panel_height(s),
            ));
        }
        rows.push(SettingsRow::item(
            SettingId::GenOpenTabInExistingInstance,
            tr("OpenTabInExistingInstance"),
            Control::Toggle(s.open_tab_in_existing_instance),
        ));
    }

    // Switch to new tab.
    rows.push(SettingsRow::card(
        SettingId::GenAlwaysSwitchToNewlyOpenedTab,
        Icon::Glyph("\u{E8AB}"),
        tr("AlwaysSwitchToNewlyOpenedTab"),
        Control::Toggle(s.always_switch_to_newly_opened_tab),
    ));

    // Widgets.
    rows.push(SettingsRow::expander(
        SettingId::GenWidgetsHeader,
        Icon::Glyph("\u{F246}"),
        tr("Widgets"),
        Control::None,
        EXP_WIDGETS,
        expanded[EXP_WIDGETS],
    ));
    if expanded[EXP_WIDGETS] {
        for (id, key, on) in [
            (SettingId::GenWidgetQuickAccess, "QuickAccess", s.show_quick_access_widget),
            (SettingId::GenWidgetDrives, "Drives", s.show_drives_widget),
            (SettingId::GenWidgetNetworkLocations, "NetworkLocations", s.show_network_locations_widget),
            (SettingId::GenWidgetFileTags, "FileTags", s.show_file_tags_widget),
            (SettingId::GenWidgetRecentFiles, "RecentFiles", s.show_recent_files_widget),
        ] {
            rows.push(SettingsRow::item(id, tr(key), Control::Toggle(on)));
        }
    }

    // Dual Pane.
    rows.push(SettingsRow::group(tr("DualPane")));
    rows.push(SettingsRow::card(
        SettingId::GenAlwaysOpenDualPane,
        Icon::Glyph("\u{E89F}"),
        tr("SettingsMultitaskingAlwaysOpenDualPane"),
        Control::Toggle(s.always_open_dual_pane_in_new_tab),
    ));
    rows.push(SettingsRow::card(
        SettingId::GenDualPaneArrangement,
        Icon::Glyph("\u{E784}"),
        tr("DualPaneSplitDirection"),
        Control::Combo(tr(s.shell_pane_arrangement.tr_key()).into()),
    ));

    // Right Click Menu.
    rows.push(SettingsRow::group(tr("SettingsContextMenu.Text")));
    rows.push(SettingsRow::expander(
        SettingId::GenContextMenuHeader,
        Icon::Glyph("\u{E74C}"),
        tr("ContextMenuOptions"),
        Control::None,
        EXP_CONTEXT_MENU,
        expanded[EXP_CONTEXT_MENU],
    ));
    if expanded[EXP_CONTEXT_MENU] {
        for (id, key, on) in [
            (SettingId::GenCtxOpenInNewTab, "ShowOpenInNewTab", s.show_open_in_new_tab),
            (SettingId::GenCtxOpenInNewWindow, "ShowOpenInNewWindow", s.show_open_in_new_window),
            (SettingId::GenCtxOpenInNewPane, "ShowOpenInNewPane", s.show_open_in_new_pane),
            (SettingId::GenCtxCopyPath, "ShowCopyPath", s.show_copy_path),
            (SettingId::GenCtxCreateFolderWithSelection, "ShowCreateFolderWithSelection", s.show_create_folder_with_selection),
            (SettingId::GenCtxCreateAlternateDataStream, "ShowCreateAlternateDataStream", s.show_create_alternate_data_stream),
            (SettingId::GenCtxCreateShortcut, "ShowCreateShortcut", s.show_create_shortcut),
            (SettingId::GenCtxPinToSideBar, "ShowPinToSideBar", s.show_pin_to_sidebar),
            (SettingId::GenCtxCompressionOptions, "ShowCompressionOptions", s.show_compression_options),
            (SettingId::GenCtxSendToMenu, "ShowSendToMenu", s.show_send_to_menu),
            (SettingId::GenCtxOpenTerminal, "ShowOpenTerminal", s.show_open_terminal),
            (SettingId::GenCtxEditTagsMenu, "ShowEditTagsMenu", s.show_edit_tags_menu),
            (SettingId::GenCtxPinToStart, "ShowPinToStart", s.show_pin_to_start),
        ] {
            rows.push(SettingsRow::item(id, tr(key), Control::Toggle(on)));
        }
    }
    rows.push(SettingsRow::card(
        SettingId::GenCtxOverflow,
        Icon::Glyph("\u{E712}"),
        tr("SettingsContextMenuOverflow"),
        Control::Toggle(s.move_shell_extensions_to_sub_menu),
    ));

    // Scrolling.
    rows.push(SettingsRow::group(tr("Scrolling")));
    rows.push(SettingsRow::card(
        SettingId::GenSmoothScrolling,
        Icon::Glyph("\u{EC8F}"),
        tr("EnableSmoothScrolling"),
        Control::Toggle(s.enable_smooth_scrolling),
    ));
    rows.push(SettingsRow::card(
        SettingId::GenTabScrollDirection,
        Icon::Glyph("\u{ECE7}"),
        tr("ReverseTabScrollDirection"),
        Control::Combo(
            tr(if s.reverse_tab_scroll_direction {
                "DownMotionScrollsRight"
            } else {
                "DownMotionScrollsLeft"
            })
            .into(),
        ),
    ));

    rows
}

/// Dropdown option lists (GeneralViewModel), in XAML order.
pub fn date_format_options(current: DateTimeFormat) -> Vec<(&'static str, bool)> {
    DateTimeFormat::ALL
        .iter()
        .map(|v| (kubuno_drive_desktop_localization::tr(v.tr_key()), *v == current))
        .collect()
}

pub fn startup_options(s: &AppSettings) -> Vec<(&'static str, bool)> {
    let tr = kubuno_drive_desktop_localization::tr;
    let current = if s.continue_last_session_on_startup {
        1
    } else if s.open_specific_page_on_startup {
        2
    } else {
        0
    };
    vec![
        (tr("SettingsOnStartupOpenANewTab.Content"), current == 0),
        (tr("SettingsOnStartupContinueWhereYouLeftOff.Content"), current == 1),
        (tr("SettingsOnStartupOpenASpecificPage.Content"), current == 2),
    ]
}

pub fn arrangement_options(current: ShellPaneArrangement) -> Vec<(&'static str, bool)> {
    ShellPaneArrangement::ALL
        .iter()
        .map(|v| (kubuno_drive_desktop_localization::tr(v.tr_key()), *v == current))
        .collect()
}

impl Painter<'_> {
    /// ItemsHeader of the Startup expander: "Ajouter une page" + pages list.
    pub(crate) fn draw_startup_pages_panel(&self, panel: &Rect) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tr = kubuno_drive_desktop_localization::tr;
        let s = crate::services::settings::get();

        // Add button (MenuFlyout: Accueil / Parcourir).
        // `@ui/Button variant="secondary" size="sm"`: `bg-white border
        // border-border text-text-primary`, `rounded-md`, never bold.
        let btn = add_page_button_rect(panel);
        self.fill_rounded(&btn, radius::SM, &t.layer_background);
        self.stroke_rounded(&btn, radius::SM, &t.card_stroke);
        let plus = Rect::new(btn.left + 8.0, btn.top, btn.left + 28.0, btn.bottom);
        self.text("\u{E710}", &plus, &f.icon_small, &t.text_primary, true);
        let label = Rect::new(btn.left + 32.0, btn.top, btn.right - 4.0, btn.bottom);
        self.text(tr("AddPage"), &label, &f.body, &t.text_primary, false);

        // Pages list with per-row remove buttons.
        for (i, path) in s.tabs_on_startup_list.iter().enumerate() {
            let row = startup_page_row_rect(panel, i);
            let display = if path == "Home" { tr("Home") } else { path.as_str() };
            let text = Rect::new(row.left + 12.0, row.top, row.right - 40.0, row.bottom);
            self.text(display, &text, &f.body, &t.text_primary, false);
            let remove = remove_page_button_rect(&row);
            self.text("\u{E74D}", &remove, &f.icon_small, &t.text_secondary, true);
        }
    }
}
