//! Port de `Files.App/Actions/Content/Selection/`.

pub mod clear_selection_action;
pub mod invert_selection_action;
pub mod select_all_action;
pub mod toggle_select_action;

pub use clear_selection_action::ClearSelection;
pub use invert_selection_action::InvertSelection;
pub use select_all_action::SelectAll;
pub use toggle_select_action::ToggleSelect;
