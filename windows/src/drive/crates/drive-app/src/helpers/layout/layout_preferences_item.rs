//! LayoutPreferences (mirrors LayoutPreferencesItem.cs)
//!
//! `LayoutPreferencesItem` — what a folder remembers about its display.
//! The original keeps display preferences **per folder** — layout, sort,
//! sort direction. The `new LayoutPreferencesItem()` constructor copies the
//! global values from `LayoutSettingsService`.

use serde::{Deserialize, Serialize};

use crate::view_models::shell_view_model::{SortColumn, ViewMode};

/// `LayoutPreferencesItem` — what a folder remembers about its display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutPreferences {
    pub layout_mode: ViewMode,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    /// `LayoutPreferencesItem.SortFilesFirst` / `SortDirectoriesAlongsideFiles`
    /// — the "folders first / files first / mixed" radio trio.
    /// (`serde(default)`: preferences saved before these fields existed.)
    #[serde(default)]
    pub sort_files_first: bool,
    #[serde(default)]
    pub sort_directories_alongside_files: bool,
    /// `DirectoryGroupOption` / `DirectoryGroupDirection`.
    #[serde(default)]
    pub group_option: crate::view_models::shell_view_model::GroupOption,
    #[serde(default = "yes")]
    pub group_ascending: bool,
    #[serde(default)]
    pub group_by_date_unit: crate::services::settings::GroupByDateUnit,
}

fn yes() -> bool {
    true
}

impl Default for LayoutPreferences {
    /// `new LayoutPreferencesItem()`: the constructor copies the global
    /// values from `LayoutSettingsService`.
    fn default() -> Self {
        let s = crate::services::settings::get();
        Self {
            layout_mode: view_mode_of(s.default_layout_mode),
            sort_column: sort_column_of(s.default_sort_option),
            sort_ascending: !s.default_sort_descending,
            sort_files_first: s.default_sort_files_first,
            sort_directories_alongside_files: s.default_sort_directories_alongside_files,
            group_option: Default::default(),
            group_ascending: !s.default_group_descending,
            group_by_date_unit: s.default_group_by_date_unit,
        }
    }
}

/// `FolderLayoutModes` → our `ViewMode`. `Adaptive` (the original's default)
/// picks the layout based on the folder's contents; not having ported it,
/// it falls back to Details — which is also what the original does when
/// adaptive is disabled (`IsAdaptiveLayoutEnabled`).
fn view_mode_of(mode: crate::services::settings::FolderLayoutMode) -> ViewMode {
    use crate::services::settings::FolderLayoutMode as F;
    match mode {
        F::Details | F::Adaptive => ViewMode::Details,
        F::List => ViewMode::List,
        F::Cards => ViewMode::Cards,
        F::Columns => ViewMode::Columns,
        F::Grid => ViewMode::Grid,
    }
}

/// `SortOption` → our sortable columns. DateCreated and FileTag don't have
/// a column here yet: they fall back to name.
fn sort_column_of(option: crate::services::settings::DefaultSortOption) -> SortColumn {
    use crate::services::settings::DefaultSortOption as D;
    match option {
        D::DateModified => SortColumn::DateModified,
        D::Size => SortColumn::Size,
        D::Type => SortColumn::Type,
        D::Name | D::DateCreated | D::FileTag => SortColumn::Name,
    }
}
