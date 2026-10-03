//! Port of `Files.App/Actions/Display/GroupAction.cs`: the `GroupByXxxAction`
//! (radio over `DirectoryGroupOption`) and `GroupAscending`/`GroupDescending`.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::GroupOption;

fn set_group(w: &mut MainWindow, option: GroupOption) {
    let tab = w.state.active_mut();
    tab.group_option = option;
    let (c, a) = (tab.sort_column, tab.sort_ascending);
    tab.set_sort(c, a); // re-sorts + rebuilds the groups
    w.state.active().save_prefs();
    w.invalidate();
}

macro_rules! group_by_action {
    ($name:ident, $option:expr, $label:literal, $desc:literal) => {
        pub struct $name;
        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                set_group(w, $option);
            }
        }
        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                w.state.active().group_option == $option
            }
        }
    };
}

group_by_action!(GroupByNone, GroupOption::None, "None", "GroupByNoneDescription");
group_by_action!(GroupByName, GroupOption::Name, "Name", "GroupByNameDescription");
group_by_action!(GroupByDateModified, GroupOption::DateModified, "DateModifiedLowerCase", "GroupByDateModifiedDescription");
group_by_action!(GroupByType, GroupOption::Type, "Type", "GroupByTypeDescription");
group_by_action!(GroupBySize, GroupOption::Size, "Size", "GroupBySizeDescription");

macro_rules! group_direction_action {
    ($name:ident, $ascending:literal, $label:literal, $desc:literal) => {
        pub struct $name;
        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn is_executable(&self, w: &MainWindow) -> bool {
                w.state.active().group_option != GroupOption::None
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                let tab = w.state.active_mut();
                tab.group_ascending = $ascending;
                let (c, a) = (tab.sort_column, tab.sort_ascending);
                tab.set_sort(c, a);
                w.state.active().save_prefs();
                w.invalidate();
            }
        }
        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                w.state.active().group_ascending == $ascending
            }
        }
    };
}

group_direction_action!(GroupAscending, true, "Ascending", "GroupAscendingDescription");
group_direction_action!(GroupDescending, false, "Descending", "GroupDescendingDescription");

/// `GroupByDateModifiedYearAction` / `…MonthAction` / `…DayAction`: the unit
/// of the date grouping (`GroupByDateUnit`), which also enables grouping by
/// modification date.
macro_rules! group_date_unit_action {
    ($name:ident, $unit:expr, $label:literal, $desc:literal) => {
        pub struct $name;
        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                {
                    let tab = w.state.active_mut();
                    tab.group_by_date_unit = $unit;
                }
                set_group(w, GroupOption::DateModified);
            }
        }
        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                let tab = w.state.active();
                tab.group_option == GroupOption::DateModified && tab.group_by_date_unit == $unit
            }
        }
    };
}

use crate::services::settings::GroupByDateUnit;
group_date_unit_action!(GroupByDateModifiedYear, GroupByDateUnit::Year, "Year", "GroupByDateModifiedYearDescription");
group_date_unit_action!(GroupByDateModifiedMonth, GroupByDateUnit::Month, "Month", "GroupByDateModifiedMonthDescription");
group_date_unit_action!(GroupByDateModifiedDay, GroupByDateUnit::Day, "Day", "GroupByDateModifiedDayDescription");
