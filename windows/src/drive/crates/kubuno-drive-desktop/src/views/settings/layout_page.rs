//! Port of `Files.App/Views/Settings/LayoutPage.xaml` + `LayoutViewModel`:
//! folder overrides, default layout type, sorting & grouping defaults and
//! the details-view columns.

use crate::services::settings::{AppSettings, DefaultGroupOption, DefaultSortOption, FolderLayoutMode, GroupByDateUnit};
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander indices in `UiState::settings_expanded[SECTION_LAYOUT]`.
pub const EXP_SORT: usize = 0;
pub const EXP_GROUP: usize = 1;
pub const EXP_COLUMNS: usize = 2;

/// `LayoutViewModel.SelectedDefaultSortPriorityIndex` labels, XAML order.
pub const SORT_PRIORITY_KEYS: [&str; 3] =
    ["SortFoldersFirst", "SortFilesFirst", "SortFilesAndFoldersTogether"];

pub fn sort_priority_index(s: &AppSettings) -> usize {
    if s.default_sort_directories_alongside_files {
        2
    } else if s.default_sort_files_first {
        1
    } else {
        0
    }
}

pub fn rows(s: &AppSettings, expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = kubuno_drive_desktop_localization::tr;
    // Folder Overrides.
    let mut rows = vec![SettingsRow::card(
        SettingId::LaySyncPreferences,
        Icon::Glyph("\u{E621}"),
        tr("SyncFolderPreferencesAcrossDirectories"),
        Control::Toggle(s.sync_folder_preferences_across_directories),
    )];

    // Layout Type.
    rows.push(SettingsRow::card(
        SettingId::LayLayoutType,
        Icon::Glyph("\u{E8BA}"),
        tr("LayoutType"),
        Control::Combo(tr(s.default_layout_mode.tr_key()).into()),
    ));

    // Sorting & grouping.
    rows.push(SettingsRow::group(tr("SortingAndGrouping")));

    // Default sorting options.
    rows.push(SettingsRow::expander(
        SettingId::LaySortByHeader,
        Icon::Glyph("\u{E8CB}"),
        tr("SortBy"),
        Control::Combo(tr(s.default_sort_option.tr_key()).into()),
        EXP_SORT,
        expanded[EXP_SORT],
    ));
    if expanded[EXP_SORT] {
        rows.push(SettingsRow::item(
            SettingId::LaySortDescending,
            tr("SortInDescendingOrder"),
            Control::Toggle(s.default_sort_descending),
        ));
        rows.push(SettingsRow::item(
            SettingId::LaySortPriority,
            tr("SortPriority"),
            Control::Combo(tr(SORT_PRIORITY_KEYS[sort_priority_index(s)]).into()),
        ));
    }

    // Default grouping options.
    rows.push(SettingsRow::expander(
        SettingId::LayGroupByHeader,
        Icon::Glyph("\u{F168}"),
        tr("GroupBy"),
        Control::Combo(tr(s.default_group_option.tr_key()).into()),
        EXP_GROUP,
        expanded[EXP_GROUP],
    ));
    if expanded[EXP_GROUP] {
        let grouped = s.default_group_option != DefaultGroupOption::None;
        rows.push(SettingsRow::item(
            SettingId::LayGroupDescending,
            tr("GroupInDescendingOrder"),
            if grouped {
                Control::Toggle(s.default_group_descending)
            } else {
                Control::ToggleDisabled(s.default_group_descending)
            },
        ));
        let by_date = s.default_group_option.is_group_by_date();
        let unit = tr(s.default_group_by_date_unit.tr_key()).to_string();
        rows.push(SettingsRow::item(
            SettingId::LayGroupByDateUnit,
            tr("GroupByDateUnit"),
            if by_date { Control::Combo(unit) } else { Control::ComboDisabled(unit) },
        ));
    }

    // Details View.
    rows.push(SettingsRow::group(tr("DetailsView")));
    rows.push(SettingsRow::card(
        SettingId::LayAutoSizeColumns,
        Icon::Glyph("\u{E784}"),
        tr("AutoSizeColumnsInDetailsLayout"),
        Control::Toggle(s.auto_size_columns_in_details_layout),
    ));
    rows.push(SettingsRow::expander(
        SettingId::LayColumnsHeader,
        Icon::Glyph("\u{E71D}"),
        tr("Columns"),
        Control::None,
        EXP_COLUMNS,
        expanded[EXP_COLUMNS],
    ));
    if expanded[EXP_COLUMNS] {
        for (id, key, on) in [
            (SettingId::LayColTag, "TagColumn", s.show_file_tag_column),
            (SettingId::LayColSize, "SizeColumn", s.show_size_column),
            (SettingId::LayColType, "TypeColumn", s.show_type_column),
            (SettingId::LayColDate, "DateColumn", s.show_date_column),
            (SettingId::LayColDateCreated, "DateCreatedColumn", s.show_date_created_column),
        ] {
            rows.push(SettingsRow::item(id, tr(key), Control::Toggle(on)));
        }
    }

    rows
}

/// LayoutType combo items; "Adaptive" is greyed while preferences are synced.
pub fn layout_mode_options(s: &AppSettings) -> Vec<(&'static str, bool, bool)> {
    FolderLayoutMode::ALL
        .iter()
        .map(|m| {
            let enabled = *m != FolderLayoutMode::Adaptive
                || !s.sync_folder_preferences_across_directories;
            (kubuno_drive_desktop_localization::tr(m.tr_key()), *m == s.default_layout_mode, enabled)
        })
        .collect()
}

pub fn sort_options(current: DefaultSortOption) -> Vec<(&'static str, bool)> {
    DefaultSortOption::ALL
        .iter()
        .map(|v| (kubuno_drive_desktop_localization::tr(v.tr_key()), *v == current))
        .collect()
}

pub fn group_options(current: DefaultGroupOption) -> Vec<(&'static str, bool)> {
    DefaultGroupOption::ALL
        .iter()
        .map(|v| (kubuno_drive_desktop_localization::tr(v.tr_key()), *v == current))
        .collect()
}

pub fn group_date_unit_options(current: GroupByDateUnit) -> Vec<(&'static str, bool)> {
    GroupByDateUnit::ALL
        .iter()
        .map(|v| (kubuno_drive_desktop_localization::tr(v.tr_key()), *v == current))
        .collect()
}
