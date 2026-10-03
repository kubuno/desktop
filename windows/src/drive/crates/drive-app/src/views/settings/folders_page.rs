//! Port of `Files.App/Views/Settings/FoldersPage.xaml` + `FoldersViewModel`:
//! display options (hidden items, extensions, thumbnails, checkboxes) and
//! behaviors (single click, delete confirmation, size format…).

use crate::services::settings::{AppSettings, DeleteConfirmationPolicy, SingleClickOpenMode, SizeUnitFormat};
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander indices in `UiState::settings_expanded[SECTION_FOLDERS]`.
pub const EXP_HIDDEN_ITEMS: usize = 0;
pub const EXP_SINGLE_CLICK: usize = 1;
/// `CalculateFolderSizes` is IsExpanded="True" in the XAML.
pub const EXP_FOLDER_SIZES: usize = 2;

pub fn rows(s: &AppSettings, expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = drive_localization::tr;
    let mut rows = Vec::new();

    // Display.
    rows.push(SettingsRow::group(tr("Display")));

    // Hidden Items (PathIcon App.Theme.PathIcon.Hide).
    rows.push(SettingsRow::expander(
        SettingId::FolHiddenItemsHeader,
        Icon::Vector("Hide"),
        tr("HiddenItems"),
        Control::None,
        EXP_HIDDEN_ITEMS,
        expanded[EXP_HIDDEN_ITEMS],
    ));
    if expanded[EXP_HIDDEN_ITEMS] {
        rows.push(SettingsRow::item(
            SettingId::FolShowHiddenItems,
            tr("SettingsFilesAndFoldersShowHiddenItems"),
            Control::Toggle(s.show_hidden_items),
        ));
        rows.push(SettingsRow::item(
            SettingId::FolShowDotFiles,
            tr("ShowDotFiles"),
            Control::Toggle(s.show_dot_files),
        ));
        rows.push(SettingsRow::item(
            SettingId::FolShowProtectedSystemFiles,
            tr("ShowProtectedSystemFiles"),
            Control::Toggle(s.show_protected_system_files),
        ));
        rows.push(SettingsRow::item(
            SettingId::FolShowAlternateStreams,
            tr("ShowAlternateStreams"),
            Control::Toggle(s.are_alternate_streams_visible),
        ));
    }

    // File Extensions.
    rows.push(SettingsRow::card(
        SettingId::FolShowFileExtensions,
        Icon::Glyph("\u{E8F9}"),
        tr("SettingsFilesAndFoldersShowFileExtensions"),
        Control::Toggle(s.show_file_extensions),
    ));

    // Show Thumbnails.
    rows.push(SettingsRow::card(
        SettingId::FolShowThumbnails,
        Icon::Glyph("\u{E91B}"),
        tr("SettingsFilesAndFoldersShowThumbnails"),
        Control::Toggle(s.show_thumbnails),
    ));

    // Show Checkboxes When Selecting Items.
    rows.push(SettingsRow::card(
        SettingId::FolShowCheckboxes,
        Icon::Glyph("\u{E73A}"),
        tr("ShowCheckboxesWhenSelectingItems"),
        Control::Toggle(s.show_checkboxes_when_selecting_items),
    ));

    // Behaviors.
    rows.push(SettingsRow::group(tr("Behaviors")));

    // Single-click to open.
    rows.push(SettingsRow::expander(
        SettingId::FolSingleClickHeader,
        Icon::Glyph("\u{ED25}"),
        tr("SingleClickToOpen"),
        Control::None,
        EXP_SINGLE_CLICK,
        expanded[EXP_SINGLE_CLICK],
    ));
    if expanded[EXP_SINGLE_CLICK] {
        rows.push(SettingsRow::item(
            SettingId::FolSingleClickFiles,
            tr("Files"),
            Control::Combo(tr(s.open_files_with_single_click.tr_key()).into()),
        ));
        rows.push(SettingsRow::item(
            SettingId::FolSingleClickFolders,
            tr("Folders"),
            Control::Combo(tr(s.open_folders_with_single_click.tr_key()).into()),
        ));
        rows.push(SettingsRow::item(
            SettingId::FolSingleClickColumnsView,
            tr("SingleClickFoldersInColumnsViewHeader"),
            Control::Combo(tr(s.open_folders_in_columns_view_with_single_click.tr_key()).into()),
        ));
    }

    // Open Folders in New Tab (inline PathIcon in the XAML).
    rows.push(SettingsRow::card(
        SettingId::FolOpenFoldersNewTab,
        Icon::Vector("OpenFoldersNewTab"),
        tr("OpenFoldersInNewTab"),
        Control::Toggle(s.open_folders_in_new_tab),
    ));

    // Confirm Delete.
    rows.push(SettingsRow::card(
        SettingId::FolDeleteConfirmation,
        Icon::Glyph("\u{E74D}"),
        tr("ShowConfirmationWhenDeletingItems"),
        Control::Combo(tr(s.delete_confirmation_policy.tr_key()).into()),
    ));

    // File Extension Warning.
    rows.push(SettingsRow::card(
        SettingId::FolExtensionWarning,
        Icon::Glyph("\u{E8AC}"),
        tr("ShowFileExtensionWarning"),
        Control::Toggle(s.show_file_extension_warning),
    ));

    // Select On Hover.
    rows.push(SettingsRow::card(
        SettingId::FolSelectOnHover,
        Icon::Glyph("\u{E8B3}"),
        tr("SelectFilesAndFoldersOnHover"),
        Control::Toggle(s.select_files_on_hover),
    ));

    // Double click to go up.
    rows.push(SettingsRow::card(
        SettingId::FolDoubleClickToGoUp,
        Icon::Glyph("\u{E8B0}"),
        tr("DoubleClickBlankSpaceToGoUp"),
        Control::Toggle(s.double_click_to_go_up),
    ));

    // Scroll to parent folder when navigating up.
    rows.push(SettingsRow::card(
        SettingId::FolScrollToPreviousFolder,
        Icon::Glyph("\u{ECE7}"),
        tr("ScrollToPreviousFolderWhenNavigatingUp"),
        Control::Toggle(s.scroll_to_previous_folder_when_navigating_up),
    ));

    // Size format.
    rows.push(SettingsRow::card(
        SettingId::FolSizeFormat,
        Icon::Glyph("\u{E67A}"),
        tr("SizeFormat"),
        Control::Combo(tr(s.size_unit_format.tr_key()).into()),
    ));

    // Calculate folder sizes (expander IsExpanded=True, InfoBar warning).
    rows.push(SettingsRow::expander(
        SettingId::FolCalculateFolderSizesHeader,
        Icon::Glyph("\u{EE40}"),
        tr("CalculateFolderSizes"),
        Control::Toggle(s.calculate_folder_sizes),
        EXP_FOLDER_SIZES,
        expanded[EXP_FOLDER_SIZES],
    ));
    if expanded[EXP_FOLDER_SIZES] {
        rows.push(SettingsRow::info_bar(tr("ShowFolderSizesWarning")));
    }

    rows
}

pub fn single_click_options(current: SingleClickOpenMode) -> Vec<(&'static str, bool)> {
    SingleClickOpenMode::ALL
        .iter()
        .map(|v| (drive_localization::tr(v.tr_key()), *v == current))
        .collect()
}

pub fn delete_confirmation_options(current: DeleteConfirmationPolicy) -> Vec<(&'static str, bool)> {
    DeleteConfirmationPolicy::ALL
        .iter()
        .map(|v| (drive_localization::tr(v.tr_key()), *v == current))
        .collect()
}

pub fn size_format_options(current: SizeUnitFormat) -> Vec<(&'static str, bool)> {
    SizeUnitFormat::ALL
        .iter()
        .map(|v| (drive_localization::tr(v.tr_key()), *v == current))
        .collect()
}
