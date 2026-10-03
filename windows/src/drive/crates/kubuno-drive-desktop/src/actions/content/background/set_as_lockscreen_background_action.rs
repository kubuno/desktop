//! SetAsLockscreenBackground (mirror of SetAsLockscreenBackgroundAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_set_as_action::{first_selected_image, set_as_executable};

/// `SetAsLockscreenBackgroundAction` (« Écran de verrouillage »).
pub struct SetAsLockscreenBackground;
impl Action for SetAsLockscreenBackground {
    fn label(&self) -> &'static str {
        "Lockscreen"
    }
    fn description(&self) -> &'static str {
        "SetAsLockscreenBackgroundDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        set_as_executable(w) && first_selected_image(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let path = parameter.map(str::to_owned).or_else(|| first_selected_image(w));
        if let Some(path) = path {
            if let Err(e) = crate::services::windows_wallpaper_service::set_lock_screen_wallpaper(&path) {
                tracing::warn!("définition du fond de l'écran de verrouillage échouée: {e}");
            }
        }
    }
}
