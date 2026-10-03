//! Port of `Files.App/Actions/Content/ImageManipulation/` (`BaseRotateAction`
//! + `RotateLeftAction` / `RotateRightAction`).
//!
//! Rotates the selected images via [`crate::helpers::bitmap_helper`],
//! then refreshes the view. `RotateLeft` = 270° clockwise, `RotateRight` = 90°.

pub mod base_rotate_action;
pub mod rotate_left_action;
pub mod rotate_right_action;

// `is_wallpaper_compatible` / `selected_images` are consumed outside this
// module (by `content::background` and `main_window::context_menus`): keep
// their `content::image_manipulation::*` path.
pub(crate) use base_rotate_action::{is_wallpaper_compatible, selected_images};
pub use rotate_left_action::RotateLeft;
pub use rotate_right_action::RotateRight;
