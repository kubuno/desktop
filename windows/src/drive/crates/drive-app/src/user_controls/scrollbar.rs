//! The `ScrollBar` was promoted to a `drive-app-controls` control
//! (`drive_app_controls::scrollbar`): struct + geometry + rendering live there.
//! This module is now just a re-export to keep the
//! `crate::user_controls::scrollbar::…` paths intact.
pub use drive_app_controls::scrollbar::*;
