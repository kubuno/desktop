//! Port of `Files.App/Actions/Display/SortAction.cs`: the `SortByXxxAction`
//! (sort column) and `SortAscending`/`SortDescending` (direction). The
//! unported columns (DateCreated, SyncStatus, FileTag, Path) will come with
//! their views.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::{Location, SortColumn};

macro_rules! sort_by_action {
    ($name:ident, $column:expr, $label:literal, $desc:literal) => {
        pub struct $name;

        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                let tab = w.state.active_mut();
                let ascending = tab.sort_ascending;
                tab.set_sort($column, ascending);
                w.state.active().save_prefs();
                w.invalidate();
            }
        }

        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                w.state.active().sort_column == $column
            }
        }
    };
}

sort_by_action!(SortByName, SortColumn::Name, "Name", "SortByNameDescription");
sort_by_action!(SortByDateModified, SortColumn::DateModified, "DateModifiedLowerCase", "SortByDateModifiedDescription");
sort_by_action!(SortBySize, SortColumn::Size, "Size", "SortBySizeDescription");
sort_by_action!(SortByType, SortColumn::Type, "Type", "SortByTypeDescription");

/// `SortByOriginalFolderAction` (SortAction.cs:120-133): sort by "Original
/// folder" — `GetIsExecutable` true only for `ContentPageTypes.RecycleBin`.
pub struct SortByOriginalFolder;

impl Action for SortByOriginalFolder {
    fn label(&self) -> &'static str {
        "OriginalFolder"
    }
    fn description(&self) -> &'static str {
        "SortByOriginalFolderDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().location == Location::RecycleBin
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let tab = w.state.active_mut();
        let ascending = tab.sort_ascending;
        tab.set_sort(SortColumn::OriginalPath, ascending);
        w.state.active().save_prefs();
        w.invalidate();
    }
}

impl ToggleAction for SortByOriginalFolder {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state.active().sort_column == SortColumn::OriginalPath
    }
}

/// `SortByDateDeletedAction` (SortAction.cs:136-149): sort by "Date
/// deleted" — `GetIsExecutable` true only for `ContentPageTypes.RecycleBin`.
pub struct SortByDateDeleted;

impl Action for SortByDateDeleted {
    fn label(&self) -> &'static str {
        "DateDeleted"
    }
    fn description(&self) -> &'static str {
        "SortByDateDeletedDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().location == Location::RecycleBin
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let tab = w.state.active_mut();
        let ascending = tab.sort_ascending;
        tab.set_sort(SortColumn::DateDeleted, ascending);
        w.state.active().save_prefs();
        w.invalidate();
    }
}

impl ToggleAction for SortByDateDeleted {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state.active().sort_column == SortColumn::DateDeleted
    }
}

macro_rules! sort_direction_action {
    ($name:ident, $ascending:literal, $label:literal, $desc:literal) => {
        pub struct $name;

        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                let tab = w.state.active_mut();
                let column = tab.sort_column;
                tab.set_sort(column, $ascending);
                w.state.active().save_prefs();
                w.invalidate();
            }
        }

        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                w.state.active().sort_ascending == $ascending
            }
        }
    };
}

sort_direction_action!(SortAscending, true, "Ascending", "SortAscendingDescription");
sort_direction_action!(SortDescending, false, "Descending", "SortDescendingDescription");
