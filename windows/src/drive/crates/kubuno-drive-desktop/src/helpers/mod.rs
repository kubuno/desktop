//! Port of `Files.App/Helpers/`.

pub mod bitmap_helper;
pub mod layout;
pub mod settings_search;
pub mod share_item_helpers;
pub mod win32;

/// Compatibility re-export: the historical `crate::helpers::layout_preferences::…`
/// API stays identical after the split into `helpers/layout/` (mirrors
/// `Helpers/Layout/LayoutPreferencesItem.cs` + `LayoutPreferencesManager.cs`).
pub mod layout_preferences {
    pub use super::layout::layout_preferences_item::LayoutPreferences;
    pub use super::layout::layout_preferences_manager::{get, set};
}
