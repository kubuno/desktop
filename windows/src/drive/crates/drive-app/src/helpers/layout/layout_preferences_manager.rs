//! LayoutPreferencesManager (mirrors LayoutPreferencesManager.cs)
//!
//! The original keeps display preferences **per folder** in a database
//! (`LayoutPreferencesDatabase`, backed by an ADS stream on the folder
//! itself). The `LayoutSettingsService.SyncFolderPreferencesAcrossDirectories`
//! setting toggles this behavior: when true, there's only a single
//! **global** set (`DefaultLayoutMode`, `DefaultSortOption`, …) shared by
//! all folders.
//!
//! We reproduce the semantics, not the plumbing: the "database" here is the
//! settings' `folder_prefs` table (normalized path → preferences),
//! serialized with the rest. The FRN (file reference number, which lets the
//! original track a renamed folder) has no useful equivalent here.

use crate::view_models::shell_view_model::{SortColumn, ViewMode};

use super::layout_preferences_item::LayoutPreferences;

fn folder_layout_of(mode: ViewMode) -> crate::services::settings::FolderLayoutMode {
    use crate::services::settings::FolderLayoutMode as F;
    match mode {
        ViewMode::Details => F::Details,
        ViewMode::List => F::List,
        ViewMode::Cards => F::Cards,
        ViewMode::Columns => F::Columns,
        ViewMode::Grid => F::Grid,
    }
}

fn sort_option_of(column: SortColumn) -> crate::services::settings::DefaultSortOption {
    use crate::services::settings::DefaultSortOption as D;
    match column {
        SortColumn::Name => D::Name,
        SortColumn::DateModified => D::DateModified,
        SortColumn::Size => D::Size,
        SortColumn::Type => D::Type,
        // Columns specific to the Recycle Bin — never persisted per folder
        // (the Recycle Bin has no path); neutral fallback.
        SortColumn::OriginalPath | SortColumn::DateDeleted => D::Name,
    }
}

/// `path.TrimPath()`: the database key is the path without a trailing
/// separator.
fn trim_path(path: &str) -> String {
    let p = path.trim_end_matches(['\\', '/']);
    // A drive root keeps its backslash (`C:\`), otherwise the key would be `C:`.
    if p.len() == 2 && p.ends_with(':') {
        format!("{p}\\")
    } else {
        p.to_ascii_lowercase()
    }
}

/// `GetLayoutPreferencesForPath`: the folder's preferences, or the global ones.
pub fn get(path: &str) -> LayoutPreferences {
    let s = crate::services::settings::get();
    if s.sync_folder_preferences_across_directories {
        return LayoutPreferences::default();
    }
    s.folder_prefs
        .get(&trim_path(path))
        .copied()
        .unwrap_or_else(LayoutPreferences::default)
}

/// `SetLayoutPreferencesForPath`: writes to the database, or to the global
/// settings when cross-folder synchronization is active.
pub fn set(path: &str, prefs: LayoutPreferences) {
    crate::services::settings::update(|s| {
        if s.sync_folder_preferences_across_directories {
            s.default_layout_mode = folder_layout_of(prefs.layout_mode);
            s.default_sort_option = sort_option_of(prefs.sort_column);
            s.default_sort_descending = !prefs.sort_ascending;
            s.default_sort_files_first = prefs.sort_files_first;
            s.default_sort_directories_alongside_files = prefs.sort_directories_alongside_files;
        } else {
            s.folder_prefs.insert(trim_path(path), prefs);
        }
    });
}
