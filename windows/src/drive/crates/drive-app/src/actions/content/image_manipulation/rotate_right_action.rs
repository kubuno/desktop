//! RotateRight (mirror of RotateRightAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use windows::Graphics::Imaging::BitmapRotation;

use super::base_rotate_action::{rotate_executable, rotate_selection};

/// `RotateRightAction` : 90° horaire.
pub struct RotateRight;
impl Action for RotateRight {
    fn label(&self) -> &'static str {
        "RotateRight"
    }
    fn description(&self) -> &'static str {
        "RotateRightDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        rotate_executable(w)
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        rotate_selection(w, BitmapRotation::Clockwise90Degrees);
    }
}
