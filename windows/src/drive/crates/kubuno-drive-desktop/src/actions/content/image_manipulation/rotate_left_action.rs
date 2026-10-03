//! RotateLeft (mirror of RotateLeftAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use windows::Graphics::Imaging::BitmapRotation;

use super::base_rotate_action::{rotate_executable, rotate_selection};

/// `RotateLeftAction` : 270° horaire.
pub struct RotateLeft;
impl Action for RotateLeft {
    fn label(&self) -> &'static str {
        "RotateLeft"
    }
    fn description(&self) -> &'static str {
        "RotateLeftDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        rotate_executable(w)
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        rotate_selection(w, BitmapRotation::Clockwise270Degrees);
    }
}
