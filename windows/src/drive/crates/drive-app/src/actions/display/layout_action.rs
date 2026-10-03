//! Port of `Files.App/Actions/Display/LayoutAction.cs`: the five
//! `ToggleLayoutAction` (Ctrl+Shift+1…5). Each toggles the current tab's
//! layout and saves it for the folder (`LayoutPreferencesManager`).
//! `LayoutAdaptive` (Ctrl+Shift+7) is not ported: adaptive layout doesn't
//! exist here yet.

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::ViewMode;

/// The common base (`ToggleLayoutAction` in C#).
macro_rules! layout_action {
    ($name:ident, $mode:expr, $label:literal, $desc:literal, $glyph:literal, $digit:literal) => {
        pub struct $name;

        impl Action for $name {
            fn label(&self) -> &'static str {
                $label
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn glyph(&self) -> Option<&'static str> {
                Some($glyph)
            }
            fn hotkey(&self) -> Option<HotKey> {
                Some(HotKey::ctrl_shift($digit as u32))
            }
            fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
                w.set_view_mode($mode);
                w.invalidate();
            }
        }

        impl ToggleAction for $name {
            fn is_on(&self, w: &MainWindow) -> bool {
                w.state.active().view_mode == $mode
            }
        }
    };
}

layout_action!(LayoutDetails, ViewMode::Details, "Details", "LayoutDetailsDescription", "LayoutDetails28", '1');
layout_action!(LayoutList, ViewMode::List, "List", "LayoutListDescription", "LayoutList28", '2');
layout_action!(LayoutCards, ViewMode::Cards, "Cards", "LayoutCardsDescription", "LayoutTiles28", '3');
layout_action!(LayoutGrid, ViewMode::Grid, "Grid", "LayoutGridDescription", "LayoutGrid28", '4');
layout_action!(LayoutColumns, ViewMode::Columns, "Columns", "LayoutColumnsDescription", "LayoutColumns28", '5');

/// `LayoutIncreaseSizeAction`: Ctrl++ enlarges the current layout's icons
/// (cycles to the next view at the upper bound).
pub struct LayoutIncreaseSize;

impl Action for LayoutIncreaseSize {
    fn label(&self) -> &'static str {
        "IncreaseSize"
    }
    fn description(&self) -> &'static str {
        "LayoutIncreaseSizeDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        // Keys.Add = VK_ADD (0x6B).
        Some(HotKey::ctrl(0x6B))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.adjust_layout_size(true);
    }
}

/// `LayoutDecreaseSizeAction`: Ctrl+- shrinks the icons (cycles to the
/// previous view at the lower bound).
pub struct LayoutDecreaseSize;

impl Action for LayoutDecreaseSize {
    fn label(&self) -> &'static str {
        "DecreaseSize"
    }
    fn description(&self) -> &'static str {
        "LayoutDecreaseSizeDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        // Keys.Subtract = VK_SUBTRACT (0x6D).
        Some(HotKey::ctrl(0x6D))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.adjust_layout_size(false);
    }
}
